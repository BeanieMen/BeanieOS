use alloc::sync::Arc;
use spin::Mutex;

use crate::storage::vfs::inode::Inode;

pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub append: bool,
}

impl OpenFlags {
    pub const READ: Self = Self {
        read: true,
        write: false,
        create: false,
        truncate: false,
        append: false,
    };

    pub const WRITE: Self = Self {
        read: false,
        write: true,
        create: false,
        truncate: false,
        append: false,
    };

    pub const RDWR: Self = Self {
        read: true,
        write: true,
        create: false,
        truncate: false,
        append: false,
    };
}

pub struct File {
    pub inode: Arc<Mutex<Inode>>,
    pub offset: u64,
    pub flags: OpenFlags,
}

impl File {
    pub fn new(inode: Arc<Mutex<Inode>>, flags: OpenFlags) -> Self {
        File {
            inode,
            offset: 0,
            flags,
        }
    }
}
