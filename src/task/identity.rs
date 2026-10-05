use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::VirtAddr;
use x86_64::registers::model_specific::GsBase;

use crate::task::thread::Thread;

pub(crate) const STACK_SIZE: usize = 4096 * 16;
pub(crate) const TIMESLICE_TICKS: u64 = 10;

#[inline]
pub(crate) fn set_current_thread(thread: *mut Thread) {
    GsBase::write(VirtAddr::new(thread as u64));
}

#[inline]
pub(crate) fn current_thread() -> *mut Thread {
    GsBase::read().as_u64() as *mut Thread
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProcessId(pub u64);

impl ProcessId {
    pub(crate) const KERNEL: ProcessId = ProcessId(0);

    pub(crate) fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        ProcessId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ThreadId(pub u64);

impl ThreadId {
    pub(crate) const MAIN: ThreadId = ThreadId(0);

    pub(crate) fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        ThreadId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Priority {
    High = 0,
    Normal = 1,
    Low = 2,
    Idle = 3,
}

impl Priority {
    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ThreadState {
    Ready,
    Running,
    Sleeping(u64),
    Blocked,
    Dead,
}

// Read off the thread GS base rather than a machine-wide atomic, which
// `Scheduler::spawn` overwrote and which every thread shared. `activate`
// publishes a new GS base before `switch_context`, so switching threads already
// switches the process identity -- nothing here is written on spawn or activate.
#[inline]
pub(crate) fn current_pid() -> ProcessId {
    let thread = current_thread();

    if thread.is_null() {
        // Before `Scheduler::init` publishes the boot context everything
        // running is the kernel process.
        return ProcessId::KERNEL;
    }

    // SAFETY: GS base holds a `*mut Thread` the scheduler published and keeps
    // alive as long as the thread can run. `pid` is written at construction and
    // never mutated, so this read cannot race.
    unsafe { (*thread).pid }
}
