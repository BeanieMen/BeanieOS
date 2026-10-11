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
    Lseek = 8,
    ArchPrctl = 158,
    SchedYield = 24,
    Getpid = 39,
    Exit = 60,
    Execve = 59,
    Poll = 7,
    Getdents64 = 217,

    // not part of the musl contract
    Mkdir = 300,
    Rmdir = 301,
    Unlink = 302,
    Create = 303,
    OpenDir = 304,

}