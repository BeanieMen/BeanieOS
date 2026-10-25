use alloc::sync::Arc;

use spin::Mutex;

use crate::storage::vfs::inode::{FileType, Inode};

const DT_UNKNOWN: u8 = 0;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

const DIRENT_HEADER: usize = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub append: bool,
    pub directory: bool,
    pub nonblock: bool,
}

impl OpenFlags {
    pub(crate) const READ_DIR: Self = Self {
        read: true,
        write: false,
        create: false,
        truncate: false,
        append: false,
        directory: true,
        nonblock: false,
    };

    pub(crate) const READ: Self = Self {
        read: true,
        write: false,
        create: false,
        truncate: false,
        append: false,
        directory: false,
        nonblock: false,
    };

    pub(crate) const WRITE: Self = Self {
        read: false,
        write: true,
        create: false,
        truncate: false,
        append: false,
        directory: false,
        nonblock: false,
    };

    pub(crate) const RDWR: Self = Self {
        read: true,
        write: true,
        create: false,
        truncate: false,
        append: false,
        directory: false,
        nonblock: false,
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

    pub(crate) fn getdents(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if self.inode.lock().file_type != FileType::Directory {
            return Err("not a directory");
        }

        let mut written = 0;

        loop {
            let entry = {
                let inode = self.inode.lock();

                inode.file_ops.readdir(&inode, self.offset)?
            };

            let Some(entry) = entry else {
                break;
            };

            self.offset += 1;

            let base = &entry.name[..8];
            let ext = &entry.name[8..11];

            let base_len = base.iter().rposition(|&b| b != b' ').map_or(0, |p| p + 1);
            let ext_len = ext.iter().rposition(|&b| b != b' ').map_or(0, |p| p + 1);

            let mut formatted = [0u8; 13];
            let mut formatted_len = 0;
            formatted[..base_len].copy_from_slice(&base[..base_len]);
            formatted_len += base_len;

            if ext_len > 0 {
                formatted[formatted_len] = b'.';
                formatted_len += 1;
                formatted[formatted_len..formatted_len + ext_len].copy_from_slice(&ext[..ext_len]);
                formatted_len += ext_len;
            }

            let name_len = formatted_len + 1;
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
            record[DIRENT_HEADER..DIRENT_HEADER + formatted_len]
                .copy_from_slice(&formatted[..formatted_len]);
            record[DIRENT_HEADER + formatted_len..reclen].fill(0);

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

        let (inode, is_dev) = {
            let inode = self.inode.lock();
            if inode.file_type != FileType::Regular && inode.file_type != FileType::Device {
                return Err("not a regular or device file");
            }
            let is_dev = inode.file_type == FileType::Device;
            (inode.clone(), is_dev)
        };

        let read = inode.file_ops.read(&inode, self.offset, buf)?;

        if !is_dev {
            self.offset += read as u64;
        }

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

        let (inode, is_dev) = {
            let inode = self.inode.lock();
            let is_dev = inode.file_type == FileType::Device;
            (inode.clone(), is_dev)
        };

        let written = if is_dev {
            let mut inode_copy = inode;
            inode_copy.file_ops.write(&mut inode_copy, offset, buf)?
        } else {
            let mut inode = self.inode.lock();
            let w = inode.file_ops.write(&mut inode, offset, buf)?;
            drop(inode);
            w
        };

        if !is_dev {
            self.offset = offset + written as u64;
        }

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
