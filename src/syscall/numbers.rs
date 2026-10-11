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
