pub(crate) type SyscallResult = Result<u64, Errno>;

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Errno {
    NoSys = 38,
    BadFd = 9,
    NoEntry = 2,
    Invalid = 22,
    NoMem = 12,
    Fault = 14,
}

pub(crate) fn encode(result: SyscallResult) -> u64 {
    match result {
        Ok(value) => value,
        Err(errno) => -(errno as i64) as u64,
    }
}
