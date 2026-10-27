use crate::syscall::entry::Regs;

pub const SIGHUP: usize = 1;
pub const SIGINT: usize = 2;
pub const SIGQUIT: usize = 3;
pub const SIGILL: usize = 4;
pub const SIGTRAP: usize = 5;
pub const SIGABRT: usize = 6;
pub const SIGBUS: usize = 7;
pub const SIGFPE: usize = 8;
pub const SIGKILL: usize = 9;
pub const SIGUSR1: usize = 10;
pub const SIGSEGV: usize = 11;
pub const SIGUSR2: usize = 12;
pub const SIGPIPE: usize = 13;
pub const SIGALRM: usize = 14;
pub const SIGTERM: usize = 15;
pub const SIGCHLD: usize = 17;
pub const SIGCONT: usize = 18;
pub const SIGSTOP: usize = 19;
pub const SIGTSTP: usize = 20;
pub const SIG_DFL: u64 = 0;
pub const SIG_IGN: u64 = 1;
pub const SA_RESTORER: u64 = 0x04000000;
pub const SA_RESTART: u64 = 0x10000000;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SigAction {
    pub sa_handler: u64,
    pub sa_flags: u64,
    pub sa_restorer: u64,
    pub sa_mask: u64,
}

impl SigAction {
    pub const fn default() -> Self {
        Self {
            sa_handler: SIG_DFL,
            sa_flags: 0,
            sa_restorer: 0,
            sa_mask: 0,
        }
    }
}

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct SignalUserContext {
    pub regs: Regs,
    pub rip: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub mask: u64,
    pub sig: u64,
}

// Process-owned state. Threads retain an owning Arc for the empty-work check;
// no raw Process pointer survives manager lookup or process reaping.
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};
use crate::arch::lock::{InterruptGuard, InterruptMutex};

pub(crate) struct SignalState {
    inner: InterruptMutex<SignalInner>,
    deliverable: AtomicBool,
}

pub(crate) struct SignalInner {
    pub pending_signals: u64,
    pub blocked_signals: u64,
    pub sigactions: [SigAction; 64],
}

impl SignalState {
    pub(crate) const fn new() -> Self {
        Self {
            inner: InterruptMutex::new(SignalInner {
                pending_signals: 0,
                blocked_signals: 0,
                sigactions: [SigAction::default(); 64],
            }),
            deliverable: AtomicBool::new(false),
        }
    }

    pub(crate) fn lock(&self) -> SignalGuard<'_> {
        SignalGuard { guard: self.inner.lock(), state: self }
    }

    #[inline]
    pub(crate) fn has_deliverable(&self) -> bool {
        self.deliverable.load(Ordering::Acquire)
    }
}

pub(crate) struct SignalGuard<'a> {
    guard: InterruptGuard<'a, SignalInner>,
    state: &'a SignalState,
}

impl Deref for SignalGuard<'_> {
    type Target = SignalInner;
    fn deref(&self) -> &Self::Target { &self.guard }
}

impl DerefMut for SignalGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.guard }
}

impl Drop for SignalGuard<'_> {
    fn drop(&mut self) {
        // Publish while still holding the state lock. A reader that sees true
        // must lock and recheck before consuming. No updater can forget this
        // publication when changing pending bits or the blocked mask.
        self.state.deliverable.store(
            (self.guard.pending_signals & !self.guard.blocked_signals) != 0,
            Ordering::Release,
        );
    }
}

// Clone only for real work / state-changing syscalls. With interrupts masked,
// scheduler-owned Thread and its Arc cannot disappear during this access.
pub(crate) fn current_state() -> Option<alloc::sync::Arc<SignalState>> {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let thread = crate::task::identity::current_thread();
        if thread.is_null() { return None; }
        // SAFETY: same scheduler lifetime contract as identity::current_pid.
        Some(unsafe { alloc::sync::Arc::clone(&(*thread).signals) })
    })
}

#[inline]
pub(crate) fn current_has_deliverable() -> bool {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let thread = crate::task::identity::current_thread();
        if thread.is_null() { return false; }
        // SAFETY: borrow ends before interrupts can permit a context switch.
        unsafe { (*thread).signals.has_deliverable() }
    })
}
