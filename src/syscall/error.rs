pub type SyscallResult = Result<u64, Errno>;

#[repr(u64)]
pub enum Errno {
    NoSys = 38,
    BadFd = 9,
    Invalid = 22,
    NoMem = 12,
}

fn encode_result(result: SyscallResult) -> u64 {
    match result {
        Ok(value) => value,
        Err(errno) => -(errno as i64) as u64,
    }
}