use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::VirtAddr;
use x86_64::registers::model_specific::GsBase;

use crate::task::thread::Thread;

pub const STACK_SIZE: usize = 4096 * 16;
pub const TIMESLICE_TICKS: u64 = 10;


#[inline]
pub fn set_current_thread(thread: *mut Thread) {
    GsBase::write(VirtAddr::new(thread as u64));
}

#[inline]
pub fn current_thread() -> *mut Thread {
    GsBase::read().as_u64() as *mut Thread
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessId(pub u64);

impl ProcessId {
    pub const KERNEL: ProcessId = ProcessId(0);

    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        ProcessId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(pub u64);

impl ThreadId {
    pub const MAIN: ThreadId = ThreadId(0);

    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        ThreadId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreadState {
    Ready,
    Running,
    Sleeping(u64),
    Blocked,
    Dead,
}

static CURRENT_PID: AtomicU64 = AtomicU64::new(0);

pub fn current_pid() -> ProcessId {
    ProcessId(CURRENT_PID.load(Ordering::Relaxed))
}

pub fn set_current_pid(pid: ProcessId) {
    CURRENT_PID.store(pid.0, Ordering::Relaxed);
}
