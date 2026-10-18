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
    pub sig: u64
    
}