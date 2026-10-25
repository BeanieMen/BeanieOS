use alloc::{collections::BTreeMap, collections::VecDeque, sync::Arc, vec::Vec};
use x86_64::VirtAddr;

use crate::arch::gdt;
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
    sleeping: Vec<(u64, ThreadId)>,
    dead_threads: Vec<ThreadId>,
    current: ThreadId,
    reschedule_requested: bool,
    initialized: bool,
    live: usize,
}

impl Scheduler {
    const fn new() -> Self {
        const EMPTY: VecDeque<ThreadId> = VecDeque::new();

        Scheduler {
            threads: BTreeMap::new(),
            ready_queues: [EMPTY; NUM_PRIORITIES],
            sleeping: Vec::new(),
            dead_threads: Vec::new(),
            current: ThreadId::MAIN,
            reschedule_requested: false,
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

    pub(crate) fn spawn_thread(&mut self, thread: Thread) -> ThreadId {
        if !self.initialized {
            self.init();
        }

        let id = thread.id;
        let pid = thread.pid;
        let priority = thread.priority;

        self.threads
            .insert(id, Arc::new(InterruptMutex::new(thread)));
        self.ready_queues[priority.index()].push_back(id);
        self.live += 1;

        crate::task::process::attach_thread(pid, id);

        id
    }

    fn sleep_for(&mut self, duration_ticks: u64) -> Option<ContextSwitch> {
        let deadline = self.ticks().saturating_add(duration_ticks);

        if let Some(current) = self.threads.get(&self.current) {
            current.lock().state = ThreadState::Sleeping(deadline);
            let idx = self.sleeping.partition_point(|&(d, _)| d <= deadline);
            self.sleeping.insert(idx, (deadline, self.current));
        }

        let switch = self.switch_to_any();
        if switch.is_none() {
            if let Some(current) = self.threads.get(&self.current) {
                current.lock().state = ThreadState::Running;
            }
            self.sleeping.retain(|&(_, id)| id != self.current);
        }
        switch
    }

    fn account_tick(&mut self) {
        self.wake_sleeping_threads();

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
        let forced = core::mem::take(&mut self.reschedule_requested);

        if forced {
            return self.switch_to_any();
        }

        self.switch_if_others_runnable()
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

        self.sleeping.retain(|&(_, sid)| sid != id);
        if !self.dead_threads.contains(&id) {
            self.dead_threads.push(id);
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
            if let Some(thread) = self.threads.get(&next_id) {
                let mut thread = thread.lock();
                thread.begin_running();
            }
            return None;
        }

        self.activate(next_id, previous_id)
    }

    fn activate(&mut self, next_id: ThreadId, previous_id: ThreadId) -> Option<ContextSwitch> {
        // Reap dead threads whose context switch has finished (cannot reap previous_id or next_id).
        let mut reaped = Vec::new();
        self.dead_threads.retain(|&dead_id| {
            if dead_id != previous_id && dead_id != next_id {
                reaped.push(dead_id);
                false
            } else {
                true
            }
        });
        for dead_id in reaped {
            self.threads.remove(&dead_id);
        }

        let (previous_slot, prev_pid, previous_fx) = {
            let previous = self.threads.get(&previous_id)?;
            let mut prev = previous.lock();
            prev.fs_base = x86_64::registers::model_specific::FsBase::read().as_u64();
            (prev.rsp_slot(), prev.pid, prev.fpu_state.data.as_mut_ptr())
        };

        let (next_rsp, next_pid, next_fx) = {
            let next = self.threads.get(&next_id)?;
            let mut next = next.lock();
            next.begin_running();
            if let Some(stack_top) = next.kernel_stack_top() {
                gdt::set_kernel_stack_top(stack_top as u64);
            }
            x86_64::registers::model_specific::FsBase::write(VirtAddr::new(next.fs_base));
            (next.saved_rsp(), next.pid, next.fpu_state.data.as_ptr())
        };

        self.current = next_id;
        self.requeue(previous_id);
        self.publish_current();

        if next_pid != prev_pid {
            crate::task::process::activate_space(next_pid);
        }

        (next_rsp != 0).then_some(ContextSwitch {
            previous_slot,
            next_rsp,
            previous_fx,
            next_fx,
        })
    }

    fn wake_sleeping_threads(&mut self) {
        let now = self.ticks();
        while let Some(&(deadline, id)) = self.sleeping.first() {
            if now < deadline {
                break;
            }
            self.sleeping.remove(0);
            if let Some(thread) = self.threads.get(&id) {
                let mut thread = thread.lock();
                if let ThreadState::Sleeping(_) = thread.state {
                    thread.state = ThreadState::Ready;
                    self.ready_queues[thread.priority.index()].push_back(id);
                }
            }
        }
    }

    fn take_next_ready(&mut self) -> Option<ThreadId> {
        self.ready_queues
            .iter_mut()
            .find_map(|queue| queue.pop_front())
    }

    fn block_current(&mut self) -> Option<ContextSwitch> {
        if let Some(current) = self.threads.get(&self.current) {
            let mut thread = current.lock();

            thread.state = ThreadState::Blocked;
        }

        let switch = self.switch_to_any();
        if switch.is_none()
            && let Some(current) = self.threads.get(&self.current)
        {
            let mut thread = current.lock();
            thread.state = ThreadState::Running;
        }
        switch
    }

    fn wake(&mut self, id: ThreadId) {
        let Some(thread) = self.threads.get(&id) else {
            return;
        };

        let mut thread = thread.lock();

        match thread.state {
            ThreadState::Blocked => {}
            ThreadState::Sleeping(_) => {
                self.sleeping.retain(|&(_, sid)| sid != id);
            }
            _ => return,
        }

        thread.state = ThreadState::Ready;

        self.ready_queues[thread.priority.index()].push_back(id);
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
    pub previous_fx: *mut u8,
    pub next_fx: *const u8,
}

static SCHEDULER: InterruptMutex<Scheduler> = InterruptMutex::new(Scheduler::new());

pub(crate) fn init() {
    SCHEDULER.lock().init();
}

pub(crate) fn spawn_in_process(
    pid: ProcessId,
    name: &str,
    entry: extern "C" fn(),
    priority: Priority,
) -> ThreadId {
    SCHEDULER.lock().spawn(pid, name, entry, priority)
}

pub(crate) fn add_forked_thread(thread: Thread) -> ThreadId {
    SCHEDULER.lock().spawn_thread(thread)
}

pub(crate) fn is_reschedule_requested() -> bool {
    SCHEDULER.lock().reschedule_requested
}

pub(crate) fn yield_now() {
    let switch = SCHEDULER.lock().yield_to();

    apply(switch);
}

pub(crate) fn yield_or_wait() {
    let switch = SCHEDULER.lock().yield_to();
    if let Some(s) = switch {
        apply(Some(s));
    } else {
        x86_64::instructions::interrupts::enable_and_hlt();
        x86_64::instructions::interrupts::disable();
    }
}

pub(crate) fn block_current() -> bool {
    let switch = SCHEDULER.lock().block_current();

    let switched = switch.is_some();

    apply(switch);

    switched
}

pub(crate) fn wake(id: ThreadId) {
    SCHEDULER.lock().wake(id);
}

pub(crate) fn sleep(duration_ticks: u64) {
    let (switch, deadline) = {
        let mut sched = SCHEDULER.lock();
        let deadline = sched.ticks().saturating_add(duration_ticks);
        let switch = sched.sleep_for(duration_ticks);
        (switch, deadline)
    };
    if let Some(switch) = switch {
        apply(Some(switch));
    } else {
        while crate::arch::interrupts::vectors::ticks() < deadline {
            x86_64::instructions::interrupts::enable_and_hlt();
            x86_64::instructions::interrupts::disable();
            SCHEDULER.lock().wake_sleeping_threads();
        }
        if let Some(current) = {
            let sched = SCHEDULER.lock();
            sched.threads.get(&sched.current).cloned()
        } {
            let mut thread = current.lock();
            thread.state = ThreadState::Running;
            thread.timeslice = TIMESLICE_TICKS;
        }
    }
}

pub(crate) fn exit() -> ! {
    let switch = SCHEDULER.lock().retire_current();

    apply(switch);

    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}

pub(crate) fn account_tick() {
    SCHEDULER.lock().account_tick();
}

fn apply(switch: Option<ContextSwitch>) {
    if let Some(ContextSwitch {
        previous_slot,
        next_rsp,
        previous_fx,
        next_fx,
    }) = switch
    {
        unsafe { switch_context(previous_slot, next_rsp, previous_fx, next_fx) };
    }
}
