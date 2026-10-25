pub(crate) type SyscallResult = Result<u64, Errno>;

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Errno {
    NoSys = 38,
    NoEntry = 2,
    Srch = 3,
    Intr = 4,
    TwoBig = 7,
    BadFd = 9,
    Child = 10,
    Again = 11,
    Invalid = 22,
    NoMem = 12,
    Fault = 14,
    NotDir = 20,
    IsDir = 21,
    NotTty = 25,
    Pipe = 32,
    Overflow = 75,
    ConnRefused = 111,
}

pub(crate) fn encode(result: SyscallResult) -> u64 {
    match result {
        Ok(value) => value,
        Err(errno) => -(errno as i64) as u64,
    }
}
