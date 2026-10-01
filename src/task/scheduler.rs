use alloc::{collections::BTreeMap, collections::VecDeque, sync::Arc};

use spin::Mutex;

use crate::arch::interrupts::vectors::ticks;
use crate::task::{
    context::switch_context,
    process::{ProcessId, PROCESS_MANAGER},
    thread::{self, Priority, Thread, ThreadId, ThreadState},
};

const NUM_PRIORITIES: usize = 4;

pub struct Scheduler {
    threads: BTreeMap<ThreadId, Arc<Mutex<Thread>>>,
    ready_queues: [VecDeque<ThreadId>; NUM_PRIORITIES],
    current: ThreadId,
    initialized: bool,
}

impl Scheduler {
    const fn new() -> Self {
        const EMPTY: VecDeque<ThreadId> = VecDeque::new();

        Scheduler {
            threads: BTreeMap::new(),
            ready_queues: [EMPTY; NUM_PRIORITIES],
            current: ThreadId::MAIN,
            initialized: false,
        }
    }

    pub fn init(&mut self) {
        if self.initialized {
            return;
        }

        let boot = Arc::new(Mutex::new(Thread::boot()));

        self.threads.insert(ThreadId::MAIN, boot);
        self.current = ThreadId::MAIN;
        self.initialized = true;
    }

    pub fn spawn(
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

        self.threads.insert(id, Arc::new(Mutex::new(thread)));
        self.ready_queues[priority.index()].push_back(id);

        if let Some(process) = PROCESS_MANAGER.lock().processes.get(&pid) {
            process.lock().attach_thread(id);
        }

        id
    }

    pub fn current_id(&self) -> ThreadId {
        self.current
    }

    pub fn tick_from_interrupt(&mut self, saved_rsp: usize) -> usize {
        self.store_current_rsp(saved_rsp);

        match self.schedule() {
            Some(ContextSwitch { next_rsp, .. }) => next_rsp,
            None => saved_rsp,
        }
    }

    pub fn yield_to(&mut self) -> Option<ContextSwitch> {
        self.schedule()
    }

    pub fn sleep_for(&mut self, duration_ticks: u64) -> Option<ContextSwitch> {
        if self.threads.len() <= 1 {
            return None;
        }

        let deadline = self.ticks().saturating_add(duration_ticks);

        if let Some(current) = self.threads.get(&self.current) {
            current.lock().state = ThreadState::Sleeping(deadline);
        }

        self.schedule()
    }

    pub fn retire_current(&mut self) -> Option<ContextSwitch> {
        let id = self.current;

        if let Some(thread) = self.threads.get(&id) {
            let pid = {
                let mut thread = thread.lock();

                thread.state = ThreadState::Dead;
                thread.pid
            };

            if let Some(process) = PROCESS_MANAGER.lock().processes.get(&pid) {
                process.lock().detach_thread(id);
            }
        }

        self.schedule()
    }

    fn store_current_rsp(&mut self, saved_rsp: usize) {
        if let Some(current) = self.threads.get(&self.current) {
            current.lock().store_rsp(saved_rsp);
        }
    }

    fn schedule(&mut self) -> Option<ContextSwitch> {
        if !self.initialized || self.threads.len() <= 1 {
            return None;
        }

        self.wake_sleeping_threads();
        self.expire_timeslice()?;

        let next_id = self.take_next_ready()?;
        let previous_id = self.current;

        if next_id == previous_id {
            self.requeue(next_id);
            return None;
        }

        self.activate(next_id, previous_id)
    }

    fn wake_sleeping_threads(&mut self) {
        let now = self.ticks();

        for thread in self.threads.values() {
            let mut thread = thread.lock();

            if let ThreadState::Sleeping(deadline) = thread.state {
                if now >= deadline {
                    thread.state = ThreadState::Ready;
                    self.ready_queues[thread.priority.index()].push_back(thread.id);
                }
            }
        }
    }

    fn expire_timeslice(&mut self) -> Option<()> {
        let current = self.threads.get(&self.current)?;

        let expired = {
            let mut current = current.lock();

            current.timeslice = current.timeslice.saturating_sub(1);

            if current.timeslice == 0 {
                current.requeue();
                true
            } else {
                false
            }
        };

        expired.then_some(())
    }

    fn take_next_ready(&mut self) -> Option<ThreadId> {
        let id = self
            .ready_queues
            .iter_mut()
            .find_map(|queue| queue.pop_front())?;

        self.requeue(id);

        Some(id)
    }

    fn requeue(&mut self, id: ThreadId) {
        if let Some(thread) = self.threads.get(&id) {
            let thread = thread.lock();

            if thread.state != ThreadState::Dead {
                self.ready_queues[thread.priority.index()].push_back(id);
            }
        }
    }

    fn activate(&mut self, next_id: ThreadId, previous_id: ThreadId) -> Option<ContextSwitch> {
        let previous_rsp = {
            let previous = self.threads.get(&previous_id)?;
            previous.lock().saved_rsp()
        };

        let (next_rsp, next_pid) = {
            let next = self.threads.get(&next_id)?;
            let mut next = next.lock();

            next.begin_running();

            (next.saved_rsp(), next.pid)
        };

        self.current = next_id;
        PROCESS_MANAGER.lock().set_current_pid(next_pid);

        (previous_rsp != 0 && next_rsp != 0).then_some(ContextSwitch {
            previous_rsp,
            next_rsp,
        })
    }

    fn ticks(&self) -> u64 {
        ticks()
    }
}

pub struct ContextSwitch {
    pub previous_rsp: usize,
    pub next_rsp: usize,
}

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

pub fn init() {
    SCHEDULER.lock().init();
}

pub fn spawn(
    name: &str,
    entry: extern "C" fn(),
    priority: Priority,
) -> ThreadId {
    let pid = PROCESS_MANAGER.lock().current_pid();

    SCHEDULER.lock().spawn(pid, name, entry, priority)
}

pub fn spawn_in_process(
    pid: ProcessId,
    name: &str,
    entry: extern "C" fn(),
    priority: Priority,
) -> ThreadId {
    SCHEDULER.lock().spawn(pid, name, entry, priority)
}

pub fn yield_now() {
    let switch = SCHEDULER.lock().yield_to();

    if let Some(ContextSwitch {
        previous_rsp,
        next_rsp,
    }) = switch
    {
        unsafe { switch_context(previous_rsp as *mut usize, next_rsp) };
    }
}

pub fn sleep(duration_ticks: u64) {
    let switch = SCHEDULER.lock().sleep_for(duration_ticks);

    if let Some(ContextSwitch {
        previous_rsp,
        next_rsp,
    }) = switch
    {
        unsafe { switch_context(previous_rsp as *mut usize, next_rsp) };
    }
}

pub fn exit() -> ! {
    let switch = SCHEDULER.lock().retire_current();

    if let Some(ContextSwitch {
        previous_rsp,
        next_rsp,
    }) = switch
    {
        unsafe { switch_context(previous_rsp as *mut usize, next_rsp) };
    }

    loop {
        x86_64::instructions::hlt();
    }
}

pub fn tick_from_interrupt(saved_rsp: usize) -> usize {
    SCHEDULER.lock().tick_from_interrupt(saved_rsp)
}
