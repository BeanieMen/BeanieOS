use alloc::sync::Arc;

use spin::Mutex;

use crate::storage::vfs::inode::{FileType, Inode};

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

    pub fn readable(&self) -> bool {
        self.flags.read || self.flags.append
    }

    pub fn writable(&self) -> bool {
        self.flags.write || self.flags.append || self.flags.truncate
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if !self.readable() {
            return Err("file is not open for reading");
        }

        let inode = self.inode.lock();

        if inode.file_type != FileType::Regular {
            return Err("not a regular file");
        }

        let read = inode.file_ops.read(&inode, self.offset, buf)?;

        drop(inode);

        self.offset += read as u64;

        Ok(read)
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str> {
        if !self.writable() {
            return Err("file is not open for writing");
        }

        let offset = if self.flags.append {
            self.inode.lock().size
        } else {
            self.offset
        };

        let mut inode = self.inode.lock();

        let ops = inode.file_ops;

        let written = ops.write(&mut inode, offset, buf)?;

        drop(inode);

        self.offset = offset + written as u64;

        Ok(written)
    }

    pub fn seek(&mut self, offset: u64) -> Result<u64, &'static str> {
        self.offset = offset;

        Ok(self.offset)
    }

    pub fn tell(&self) -> u64 {
        self.offset
    }
}
