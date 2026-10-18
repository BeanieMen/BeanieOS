macro_rules! syscalls {
    ($($name:ident = $num:expr),* $(,)?) => {
        #[repr(u64)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum Syscall {
            $($name = $num),*
        }

        impl TryFrom<u64> for Syscall {
            type Error = ();

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                match value {
                    $($num => Ok(Self::$name),)*
                    _ => Err(()),
                }
            }
        }
    };
}

syscalls! {
    // Linux x86_64
    Read = 0,
    Write = 1,
    Open = 2,
    Close = 3,
    Fstat = 5,
    Lseek = 8,
    Mmap = 9,
    Munmap = 11,
    Brk = 12,
    Ioctl = 16,
    Writev = 20,
    ArchPrctl = 158,
    SchedYield = 24,
    Nanosleep = 35,
    Getpid = 39,
    Fork = 57,
    Execve = 59,
    Exit = 60,
    Wait4 = 61,
    Poll = 7,
    RtSigaction = 13,
    RtSigprocmask = 14,
    RtSigreturn = 15,
    Kill = 62,
    
    Getdents64 = 217,
    SetTidAddress = 218,
    ExitGroup = 231,
    
    Mkdir = 83,
    Rmdir = 84,
    Create = 85,
    Unlink = 87,
}