use core::{
    arch::naked_asm,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::Arc,
    vec,
};
use spin::Mutex;

use core::ops::Add;

use crate::{
    arch::interrupts::vectors::ticks,
    task::process::{ProcessId, PROCESS_MANAGER},
};



const STACK_SIZE: usize = 4096 * 16;
const NUM_PRIORITIES: usize = 4;
const TIMESLICE_TICKS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(u64);

impl ThreadId {
    pub const MAIN: ThreadId = ThreadId(0);

    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        ThreadId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High = 0,
    Normal = 1,
    Low = 2,
    Idle = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreadState {
    Ready,
    Running,
    Sleeping(u64),
    Blocked,
    Dead,
}

pub struct Thread {
    pub id: ThreadId,
    pub pid: ProcessId,
    pub name: String,
    pub state: ThreadState,
    pub priority: Priority,
    pub rsp: usize,
    pub stack: Option<Box<[u8]>>,
    pub timeslice: u64,
}

impl Thread {
    pub fn new_main() -> Self {
        Thread {
            id: ThreadId::MAIN,
            pid: ProcessId::KERNEL,
            name: String::from("main"),
            state: ThreadState::Running,
            priority: Priority::Normal,
            rsp: 0,
            stack: None,
            timeslice: TIMESLICE_TICKS,
        }
    }

    pub fn new(
        pid: ProcessId,
        name: &str,
        entry: extern "C" fn(),
        priority: Priority,
    ) -> Self {
        let id = ThreadId::new();
        let stack = vec![0u8; STACK_SIZE].into_boxed_slice();
        let stack_top = stack.as_ptr() as usize + STACK_SIZE;

        let mut sp = stack_top & !0xf;

        sp -= 8;
        unsafe {
            *(sp as *mut usize) = thread_trampoline_exit as usize;
        }

        sp -= 8;
        unsafe {
            *(sp as *mut usize) = entry as usize;
        }

        sp -= 7 * 8;
        unsafe {
            core::ptr::write_bytes(sp as *mut u8, 0, 7 * 8);

            let rflags_ptr = sp.add(6 * 8) as *mut usize;
            *rflags_ptr = 0x200;
        }

        Thread {
            id,
            pid,
            name: String::from(name),
            state: ThreadState::Ready,
            priority,
            rsp: sp,
            stack: Some(stack),
            timeslice: TIMESLICE_TICKS,
        }
    }
}

pub struct Scheduler {
    threads: BTreeMap<ThreadId, Arc<Mutex<Thread>>>,
    ready_queues: [VecDeque<ThreadId>; NUM_PRIORITIES],
    current: ThreadId,
    initialized: bool,
}

impl Scheduler {
    const fn new() -> Self {
        const EMPTY_QUEUE: VecDeque<ThreadId> = VecDeque::new();

        Scheduler {
            threads: BTreeMap::new(),
            ready_queues: [EMPTY_QUEUE; NUM_PRIORITIES],
            current: ThreadId::MAIN,
            initialized: false,
        }
    }

    pub fn init(&mut self) {
        if self.initialized {
            return;
        }

        let main = Arc::new(Mutex::new(Thread::new_main()));

        self.threads.insert(ThreadId::MAIN, main);
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
        let tid = thread.id;
        let prio_idx = priority as usize;
        let arc = Arc::new(Mutex::new(thread));

        self.threads.insert(tid, arc);
        self.ready_queues[prio_idx].push_back(tid);

        if let Some(proc_arc) = PROCESS_MANAGER.lock().processes.get(&pid) {
            proc_arc.lock().attach_thread(tid);
        }

        tid
    }

    pub fn ticks(&self) -> u64 {
        ticks()
    }

    pub fn tick(&mut self) -> Option<(*mut usize, usize)> {
        let current = self
            .threads
            .get(&self.current)
            .expect("scheduler current thread does not exist");

        let mut thread = current.lock();

        if thread.timeslice > 0 {
            thread.timeslice -= 1;
        }

        if thread.timeslice != 0 {
            return None;
        }

        thread.timeslice = TIMESLICE_TICKS;

        drop(thread);

        self.pick_next()
    }

    pub fn pick_next(&mut self) -> Option<(*mut usize, usize)> {
        if !self.initialized || self.threads.len() <= 1 {
            return None;
        }

        let now = self.ticks();

        for arc in self.threads.values() {
            let mut th = arc.lock();

            if let ThreadState::Sleeping(wake_tick) = th.state {
                if now >= wake_tick {
                    th.state = ThreadState::Ready;

                    let prio = th.priority as usize;
                    self.ready_queues[prio].push_back(th.id);
                }
            }
        }

        let mut next_tid = None;

        for queue in &mut self.ready_queues {
            if let Some(tid) = queue.pop_front() {
                next_tid = Some(tid);
                break;
            }
        }

        let next_tid = match next_tid {
            Some(id) => id,
            None => return None,
        };

        if next_tid == self.current {
            let prio = self
                .threads
                .get(&next_tid)
                .map(|t| t.lock().priority as usize);

            if let Some(prio) = prio {
                self.ready_queues[prio].push_back(next_tid);
            }

            return None;
        }

        let mut prev_rsp_ptr: *mut usize = core::ptr::null_mut();
        let mut next_rsp = 0;
        let mut next_pid = ProcessId::KERNEL;

        if let Some(curr_arc) = self.threads.get(&self.current) {
            let mut curr = curr_arc.lock();

            if curr.state == ThreadState::Running {
                curr.state = ThreadState::Ready;

                let prio = curr.priority as usize;
                self.ready_queues[prio].push_back(curr.id);
            }

            curr.timeslice = TIMESLICE_TICKS;

            prev_rsp_ptr = &mut curr.rsp as *mut usize;
        }

        if let Some(next_arc) = self.threads.get(&next_tid) {
            let mut next = next_arc.lock();

            next.state = ThreadState::Running;
            next.timeslice = TIMESLICE_TICKS;

            next_rsp = next.rsp;
            next_pid = next.pid;
        }

        self.current = next_tid;
        PROCESS_MANAGER.lock().set_current_pid(next_pid);

        if !prev_rsp_ptr.is_null() && next_rsp != 0 {
            Some((prev_rsp_ptr, next_rsp))
        } else {
            None
        }
    }

    pub fn sleep_for(
        &mut self,
        duration_ticks: u64,
    ) -> Option<(*mut usize, usize)> {
        let deadline = self.ticks().saturating_add(duration_ticks);

        if let Some(curr_arc) = self.threads.get(&self.current) {
            curr_arc.lock().state = ThreadState::Sleeping(deadline);
        }

        self.pick_next()
    }

    pub fn mark_dead_and_pick_next(
        &mut self,
    ) -> Option<(*mut usize, usize)> {
        let tid = self.current;

        if let Some(arc) = self.threads.get(&tid) {
            let mut th = arc.lock();

            th.state = ThreadState::Dead;

            let pid = th.pid;

            drop(th);

            if let Some(proc_arc) = PROCESS_MANAGER.lock().processes.get(&pid) {
                proc_arc.lock().detach_thread(tid);
            }
        }

        self.pick_next()
    }

    pub fn current_id(&self) -> ThreadId {
        self.current
    }
}

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

#[unsafe(naked)]
pub unsafe extern "C" fn switch_context(
    old_rsp: *mut usize,
    new_rsp: usize,
) {
    naked_asm!(
        "pushfq",
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push rbx",
        "push rbp",

        "mov [rdi], rsp",

        "mov rsp, rsi",

        "pop rbp",
        "pop rbx",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        "popfq",

        "ret",
    );
}

extern "C" fn thread_trampoline_exit() {
    exit()
}

fn switch_with_lock_dropped(
    switch: Option<(*mut usize, usize)>,
) {
    if let Some((prev, next)) = switch {
        unsafe {
            switch_context(prev, next);
        }
    }
}

pub fn init() {
    SCHEDULER.lock().init();
}

pub fn spawn(
    name: &str,
    entry: extern "C" fn(),
    priority: Priority,
) -> ThreadId {
    let pid = PROCESS_MANAGER.lock().current_pid();

    SCHEDULER
        .lock()
        .spawn(pid, name, entry, priority)
}

pub fn spawn_in_process(
    pid: ProcessId,
    name: &str,
    entry: extern "C" fn(),
    priority: Priority,
) -> ThreadId {
    SCHEDULER
        .lock()
        .spawn(pid, name, entry, priority)
}

pub fn yield_now() {
    let switch = {
        SCHEDULER.lock().pick_next()
    };

    switch_with_lock_dropped(switch);
}

pub fn sleep(duration_ticks: u64) {
    let switch = {
        SCHEDULER.lock().sleep_for(duration_ticks)
    };

    switch_with_lock_dropped(switch);
}

pub fn exit() -> ! {
    let switch = {
        SCHEDULER.lock().mark_dead_and_pick_next()
    };

    switch_with_lock_dropped(switch);

    loop {
        x86_64::instructions::hlt();
    }
}

pub fn tick() {
    let switch = {
        SCHEDULER.lock().tick()
    };

    switch_with_lock_dropped(switch);
}