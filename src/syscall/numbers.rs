#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    Exit = 0,
    Write = 1,
    Read = 2,
    Open = 3,
    Close = 4,
    Mkdir = 5,
    Getdents = 6,
    Yield = 7,
    Getpid = 8,
    Unlink = 9,
    Rmdir = 10,
}

impl TryFrom<u64> for Syscall {
    type Error = ();

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Exit),
            1 => Ok(Self::Write),
            2 => Ok(Self::Read),
            3 => Ok(Self::Open),
            4 => Ok(Self::Close),
            5 => Ok(Self::Mkdir),
            6 => Ok(Self::Getdents),
            7 => Ok(Self::Yield),
            8 => Ok(Self::Getpid),
            9 => Ok(Self::Unlink),
            10 => Ok(Self::Rmdir),
            _ => Err(()),
        }
    }
}
