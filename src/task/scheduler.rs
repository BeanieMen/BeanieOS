use alloc::{collections::BTreeMap, collections::VecDeque, sync::Arc, vec::Vec};

use crate::arch::interrupts::vectors::ticks;
use crate::arch::lock::InterruptMutex;
use crate::task::{
    identity::{self, Priority, ProcessId, TIMESLICE_TICKS, ThreadId, ThreadState},
    thread::{Thread, switch_context},
};

const NUM_PRIORITIES: usize = 4;

struct Scheduler {
    threads: BTreeMap<ThreadId, Arc<InterruptMutex<Thread>>>,
    ready_queues: [VecDeque<ThreadId>; NUM_PRIORITIES],
    current: ThreadId,
    reschedule_requested: bool,
    charged_up_to: u64,
    initialized: bool,
    // Threads in `threads` that are not `Dead`. Counted on entry and decremented
    // when a thread turns `Dead`, so `remove` only drops already-counted ones.
    live: usize,
}

impl Scheduler {
    const fn new() -> Self {
        const EMPTY: VecDeque<ThreadId> = VecDeque::new();

        Scheduler {
            threads: BTreeMap::new(),
            ready_queues: [EMPTY; NUM_PRIORITIES],
            current: ThreadId::MAIN,
            reschedule_requested: false,
            charged_up_to: 0,
            initialized: false,
            live: 0,
        }
    }

    pub(crate) fn init(&mut self) {
        if self.initialized {
            return;
        }

        let boot = Arc::new(InterruptMutex::new(Thread::boot()));

        self.threads.insert(ThreadId::MAIN, boot);
        self.current = ThreadId::MAIN;
        self.live = 1;

        // The boot context never had an `activate` call, so publish it here.
        self.publish_current();

        self.initialized = true;
    }

    fn thread_ptr(&self, id: ThreadId) -> Option<*mut Thread> {
        let thread = self.threads.get(&id)?;

        let thread_ptr = {
            let mut thread = thread.lock();
            &mut *thread as *mut Thread
        };

        Some(thread_ptr)
    }

    fn publish_current(&mut self) {
        if let Some(current_ptr) = self.thread_ptr(self.current) {
            identity::set_current_thread(current_ptr);
        }
    }

    fn gs_base_agrees(&self) -> bool {
        self.thread_ptr(self.current)
            .is_some_and(|current_ptr| identity::current_thread() == current_ptr)
    }

    pub(crate) fn spawn(
        &mut self,
        pid: ProcessId,
        name: &str,
        entry: extern "C" fn(),
        priority: Priority,
    ) -> ThreadId {
        if !self.initialized {
            self.init();
        }

        let thread = Thread::new(pid, name, entry, priority);
        let id = thread.id;

        self.threads
            .insert(id, Arc::new(InterruptMutex::new(thread)));
        self.ready_queues[priority.index()].push_back(id);
        self.live += 1;

        crate::task::process::attach_thread(pid, id);

        id
    }

    fn current_id(&self) -> ThreadId {
        self.current
    }

    // From the thread GS base, not this map: `self.current` only agrees with GS base
    // between switches, and asking the map meant taking SCHEDULER plus every
    // thread lock to answer a question GS base already holds.
    pub(crate) fn current_pid(&self) -> ProcessId {
        identity::current_pid()
    }

    fn account_tick(&mut self) {
        let Some(current) = self.threads.get(&self.current) else {
            return;
        };

        let expired = {
            let mut current = current.lock();

            current.timeslice = current.timeslice.saturating_sub(1);

            if current.timeslice == 0 {
                current.timeslice = TIMESLICE_TICKS;
                true
            } else {
                false
            }
        };

        if expired {
            self.reschedule_requested = true;
        }
    }

    fn yield_to(&mut self) -> Option<ContextSwitch> {
        self.charge_ticks();

        let forced = core::mem::take(&mut self.reschedule_requested);

        if forced {
            return self.switch_to_any();
        }

        self.switch_if_others_runnable()
    }

    fn charge_ticks(&mut self) {
        let now = self.ticks();
        let elapsed = now.saturating_sub(self.charged_up_to);
        self.charged_up_to = now;

        let Some(current) = self.threads.get(&self.current) else {
            return;
        };

        let mut current = current.lock();

        if current.timeslice <= elapsed {
            current.timeslice = TIMESLICE_TICKS;
            self.reschedule_requested = true;
        } else {
            current.timeslice -= elapsed;
        }
    }

    fn sleep_for(&mut self, duration_ticks: u64) -> Option<ContextSwitch> {
        if self.runnable_count() <= 1 {
            return None;
        }

        let deadline = self.ticks().saturating_add(duration_ticks);

        if let Some(current) = self.threads.get(&self.current) {
            current.lock().state = ThreadState::Sleeping(deadline);
        }

        self.switch_to_any()
    }

    fn retire_current(&mut self) -> Option<ContextSwitch> {
        let id = self.current;

        if let Some(thread) = self.threads.get(&id) {
            let mut guard = thread.lock();

            if guard.state != ThreadState::Dead {
                guard.state = ThreadState::Dead;
                self.live = self.live.saturating_sub(1);
            }
        }

        self.switch_to_any()
    }

    fn switch_if_others_runnable(&mut self) -> Option<ContextSwitch> {
        if !self.initialized {
            return None;
        }

        self.wake_sleeping_threads();

        if self.runnable_count() <= 1 {
            return None;
        }

        self.activate_next()
    }

    fn switch_to_any(&mut self) -> Option<ContextSwitch> {
        if !self.initialized {
            return None;
        }

        self.wake_sleeping_threads();

        self.activate_next()
    }

    fn activate_next(&mut self) -> Option<ContextSwitch> {
        let next_id = self.take_next_ready()?;
        let previous_id = self.current;

        if next_id == previous_id {
            // `activate` was skipped, so re-publish the pointer GS base has.
            self.requeue(next_id);

            return None;
        }

        self.activate(next_id, previous_id)
    }

    fn activate(&mut self, next_id: ThreadId, previous_id: ThreadId) -> Option<ContextSwitch> {
        let previous_slot = {
            let previous = self.threads.get(&previous_id)?;
            previous.lock().rsp_slot()
        };

        let next_rsp = {
            let next = self.threads.get(&next_id)?;
            let mut next = next.lock();

            next.begin_running();

            next.saved_rsp()
        };

        self.current = next_id;
        self.requeue(previous_id);

        // Must land before `apply` runs `switch_context`. Publishing the new
        // thread is also what publishes its pid: `identity::current_pid` reads
        // it off the thread GS base points at, so there is no second copy of
        // the current pid to keep in step here.
        self.publish_current();

        (next_rsp != 0).then_some(ContextSwitch {
            previous_slot,
            next_rsp,
        })
    }

    fn remove(&mut self, id: ThreadId) -> bool {
        let Some(thread) = self.threads.get(&id) else {
            return false;
        };

        let (pid, ptr) = {
            let mut guard = thread.lock();
            (guard.pid, &mut *guard as *mut Thread)
        };

        // The running thread's slot is where its stack pointer is parked when it
        // is switched out, and it is written again on the way back. Dropping the
        // entry now would leave that write landing in freed memory.
        if identity::current_thread() == ptr {
            return false;
        }

        if self.threads.remove(&id).is_none() {
            return false;
        }

        crate::task::process::detach_thread(pid, id);

        true
    }

    fn reap_dead(&mut self) {
        let dead: Vec<ThreadId> = self
            .threads
            .iter()
            .filter(|(_, thread)| thread.lock().state == ThreadState::Dead)
            .map(|(id, _)| *id)
            .collect();

        for id in dead {
            self.remove(id);
        }
    }

    fn wake_sleeping_threads(&mut self) {
        let now = self.ticks();

        for thread in self.threads.values() {
            let mut thread = thread.lock();

            if let ThreadState::Sleeping(deadline) = thread.state
                && now >= deadline
            {
                thread.state = ThreadState::Ready;
                self.ready_queues[thread.priority.index()].push_back(thread.id);
            }
        }
    }

    fn take_next_ready(&mut self) -> Option<ThreadId> {
        self.ready_queues
            .iter_mut()
            .find_map(|queue| queue.pop_front())
    }

    fn requeue(&mut self, id: ThreadId) {
        let Some(thread) = self.threads.get(&id) else {
            return;
        };

        let mut thread = thread.lock();

        match thread.state {
            ThreadState::Ready => {}
            ThreadState::Running => thread.requeue(),
            ThreadState::Dead | ThreadState::Blocked | ThreadState::Sleeping(_) => return,
        }

        self.ready_queues[thread.priority.index()].push_back(id);
    }

    fn runnable_count(&self) -> usize {
        debug_assert_eq!(self.live, self.count_live_by_scan(), "live counter drifted");

        self.live
    }

    // `debug_assert_eq!` still typechecks its arguments in a release build, so
    // this cannot be `#[cfg]`-gated out; mark it unused instead.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    fn count_live_by_scan(&self) -> usize {
        self.threads
            .values()
            .filter(|thread| thread.lock().state != ThreadState::Dead)
            .count()
    }

    fn ticks(&self) -> u64 {
        ticks()
    }
}

struct ContextSwitch {
    pub previous_slot: *mut usize,
    pub next_rsp: usize,
}

static SCHEDULER: InterruptMutex<Scheduler> = InterruptMutex::new(Scheduler::new());

pub(crate) fn init() {
    SCHEDULER.lock().init();
}

pub fn spawn(name: &str, entry: extern "C" fn(), priority: Priority) -> ThreadId {
    let pid = identity::current_pid();

    SCHEDULER.lock().spawn(pid, name, entry, priority)
}

pub(crate) fn spawn_in_process(
    pid: ProcessId,
    name: &str,
    entry: extern "C" fn(),
    priority: Priority,
) -> ThreadId {
    SCHEDULER.lock().spawn(pid, name, entry, priority)
}

pub(crate) fn yield_now() {
    let switch = SCHEDULER.lock().yield_to();

    apply(switch);
}

pub(crate) fn sleep(duration_ticks: u64) {
    let switch = SCHEDULER.lock().sleep_for(duration_ticks);

    apply(switch);
}

pub(crate) fn exit() -> ! {
    let switch = SCHEDULER.lock().retire_current();

    apply(switch);

    loop {
        x86_64::instructions::hlt();
    }
}

fn account_tick() {
    SCHEDULER.lock().account_tick();
}

fn reap_dead() {
    SCHEDULER.lock().reap_dead();
}

fn apply(switch: Option<ContextSwitch>) {
    if let Some(ContextSwitch {
        previous_slot,
        next_rsp,
    }) = switch
    {
        unsafe { switch_context(previous_slot, next_rsp) };
    }
}
