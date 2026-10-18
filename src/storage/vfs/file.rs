use alloc::sync::Arc;

use spin::Mutex;

use crate::storage::vfs::inode::{FileType, Inode};

const DT_UNKNOWN: u8 = 0;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

/// Header of a directory record: ino, next offset, record length, type.
const DIRENT_HEADER: usize = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub append: bool,
    pub directory: bool,
}

impl OpenFlags {
    pub(crate) const READ_DIR: Self = Self {
        read: true,
        write: false,
        create: false,
        truncate: false,
        append: false,
        directory: true,
    };

    pub(crate) const READ: Self = Self {
        read: true,
        write: false,
        create: false,
        truncate: false,
        append: false,
        directory: false,
    };

    pub(crate) const WRITE: Self = Self {
        read: false,
        write: true,
        create: false,
        truncate: false,
        append: false,
        directory: false,
    };

    pub(crate) const RDWR: Self = Self {
        read: true,
        write: true,
        create: false,
        truncate: false,
        append: false,
        directory: false,
    };
}

#[derive(Clone)]
pub(crate) struct File {
    pub inode: Arc<Mutex<Inode>>,
    pub offset: u64,
    pub flags: OpenFlags,
}

impl File {
    pub(crate) fn new(inode: Arc<Mutex<Inode>>, flags: OpenFlags) -> Self {
        File {
            inode,
            offset: 0,
            flags,
        }
    }

    /// Fills `buf` with `linux_dirent64`-shaped records, one per entry.
    ///
    /// The file's offset is the index of the next entry, so a caller that keeps
    /// reading picks up where the last full buffer ended.
    pub(crate) fn getdents(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if self.inode.lock().file_type != FileType::Directory {
            return Err("not a directory");
        }

        let mut written = 0usize;

        loop {
            let entry = {
                let inode = self.inode.lock();

                inode.file_ops.readdir(&inode, self.offset)?
            };

            let Some(entry) = entry else {
                break;
            };

            self.offset += 1;

            let name_len = entry.name.iter().position(|b| *b == b' ').unwrap_or(11) + 1;

            // align record as linux abi
            let reclen = (DIRENT_HEADER + name_len + 7) & !7;
            
            if written + reclen > buf.len() {
                if written == 0 {
                    return Err("buffer too small for one entry");
                }

                break;
            }

            let d_type = match entry.file_type {
                FileType::Directory => DT_DIR,
                FileType::Regular => DT_REG,
                _ => DT_UNKNOWN,
            };

            let record = &mut buf[written..written + reclen];

            record[0..8].copy_from_slice(&entry.inode.to_le_bytes());
            record[8..16].copy_from_slice(&self.offset.to_le_bytes());
            record[16..18].copy_from_slice(&(reclen as u16).to_le_bytes());
            record[18] = d_type;
            record[DIRENT_HEADER..DIRENT_HEADER + name_len - 1]
                .copy_from_slice(&entry.name[..name_len - 1]);
            record[DIRENT_HEADER + name_len - 1..reclen].fill(0);

            written += reclen;
        }

        Ok(written)
    }

    pub(crate) fn readable(&self) -> bool {
        self.flags.read || self.flags.append
    }

    pub fn writable(&self) -> bool {
        self.flags.write || self.flags.append || self.flags.truncate
    }

    pub(crate) fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
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

    pub(crate) fn write(&mut self, buf: &[u8]) -> Result<usize, &'static str> {
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

    pub(crate) fn seek(&mut self, offset: u64) -> Result<u64, &'static str> {
        self.offset = offset;

        Ok(self.offset)
    }

    pub(crate) fn tell(&self) -> u64 {
        self.offset
    }

    pub(crate) fn size(&self) -> u64 {
        self.inode.lock().size
    }
}
