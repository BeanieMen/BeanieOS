use alloc::sync::Arc;
use spin::Mutex;

use crate::storage::vfs::file::{File, OpenFlags};
use crate::storage::vfs::inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry};

pub struct TtyInodeOps;

impl InodeOperations for TtyInodeOps {
    fn lookup(&self, _: &Inode, _: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not a directory")
    }

    fn create(
        &self,
        _: &mut Inode,
        _: &[u8],
        _: FileType,
    ) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not supported on tty")
    }

    fn unlink(&self, _: &mut Inode, _: &[u8]) -> Result<(), &'static str> {
        Err("not supported on tty")
    }

    fn mkdir(&self, _: &mut Inode, _: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not supported on tty")
    }

    fn rmdir(&self, _: &mut Inode, _: &[u8]) -> Result<(), &'static str> {
        Err("not supported on tty")
    }
}

pub struct TtyFileOps;

impl FileOperations for TtyFileOps {
    fn read(&self, _inode: &Inode, _offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut count = 0;
        while count == 0 {
            count = crate::tty::GLOBAL_TTY.lock().read(buf);
            if count > 0 {
                return Ok(count);
            }
            if crate::task::process::has_pending_signals() {
                return Err("interrupted");
            }
            crate::syscall::dispatch::wait_for_key();
        }
        Ok(count)
    }

    fn write(&self, _inode: &mut Inode, _offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(crate::tty::GLOBAL_TTY.lock().write(buf))
    }

    fn readdir(&self, _inode: &Inode, _offset: u64) -> Result<Option<RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub static TTY_INODE_OPS: TtyInodeOps = TtyInodeOps;
pub static TTY_FILE_OPS: TtyFileOps = TtyFileOps;

pub fn create_tty_file(flags: OpenFlags) -> File {
    let inode = Inode {
        id: 1,
        file_type: FileType::Device,
        size: 0,
        cluster: 0,
        inode_ops: &TTY_INODE_OPS,
        file_ops: &TTY_FILE_OPS,
        dentry: None,
    };
    File::new(Arc::new(Mutex::new(inode)), flags)
}
