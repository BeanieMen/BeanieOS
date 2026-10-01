use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{boxed::Box, string::String, vec};

pub const STACK_SIZE: usize = 4096 * 16;
pub const TIMESLICE_TICKS: u64 = 10;

const RBP_WORD: usize = 0;
const RBX_WORD: usize = 1;
const R12_WORD: usize = 2;
const R13_WORD: usize = 3;
const R14_WORD: usize = 4;
const R15_WORD: usize = 5;
const RFLAGS_WORD: usize = 6;
const RIP_WORD: usize = 7;
const RETURN_WORD: usize = 8;

const FRAME_WORDS: usize = RETURN_WORD + 1;
const FRAME_BYTES: usize = FRAME_WORDS * 8;

const RFLAGS_IF: usize = 0x202;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(pub u64);

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

impl Priority {
    pub fn index(self) -> usize {
        self as usize
    }
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
    pub pid: crate::task::process::ProcessId,
    pub name: String,
    pub state: ThreadState,
    pub priority: Priority,
    pub stack: Option<Box<[u8]>>,
    pub timeslice: u64,
    rsp: UnsafeCell<usize>,
}

impl Thread {
    pub fn boot() -> Self {
        Thread {
            id: ThreadId::MAIN,
            pid: crate::task::process::ProcessId::KERNEL,
            name: String::from("main"),
            state: ThreadState::Running,
            priority: Priority::Normal,
            rsp: UnsafeCell::new(0),
            stack: None,
            timeslice: TIMESLICE_TICKS,
        }
    }

    pub fn new(
        pid: crate::task::process::ProcessId,
        name: &str,
        entry: extern "C" fn(),
        priority: Priority,
    ) -> Self {
        let stack = vec![0u8; STACK_SIZE].into_boxed_slice();
        let stack_top = (stack.as_ptr() as usize + STACK_SIZE) & !0xf;
        let sp = stack_top - FRAME_BYTES;

        build_initial_frame(sp, entry);

        Thread {
            id: ThreadId::new(),
            pid,
            name: String::from(name),
            state: ThreadState::Ready,
            priority,
            rsp: UnsafeCell::new(sp),
            stack: Some(stack),
            timeslice: TIMESLICE_TICKS,
        }
    }

    pub fn saved_rsp(&self) -> usize {
        unsafe { *self.rsp.get() }
    }

    pub fn rsp_slot(&self) -> *mut usize {
        self.rsp.get()
    }

    pub fn requeue(&mut self) {
        self.state = ThreadState::Ready;
        self.timeslice = TIMESLICE_TICKS;
    }

    pub fn begin_running(&mut self) {
        self.state = ThreadState::Running;
        self.timeslice = TIMESLICE_TICKS;
    }
}

fn build_initial_frame(sp: usize, entry: extern "C" fn()) {
    debug_assert_eq!(sp % 16, 8);
    debug_assert_eq!((sp + RIP_WORD * 8) % 16, 0);

    let frame = sp as *mut usize;

    unsafe {
        core::ptr::write_bytes(frame, 0, FRAME_WORDS);

        frame.add(RFLAGS_WORD).write(RFLAGS_IF);
        frame.add(RIP_WORD).write(entry as usize);
        frame.add(RETURN_WORD).write(crate::task::context::thread_trampoline_exit() as usize);
    }
}
