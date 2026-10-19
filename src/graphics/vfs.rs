use alloc::sync::Arc;
use spin::Mutex;

use crate::storage::vfs::{
    file::{File, OpenFlags},
    inode::{FileOperations, FileType, Inode, InodeOperations},
};

pub struct FbInodeOps;
impl InodeOperations for FbInodeOps {
    fn lookup(
        &self,
        _inode: &crate::storage::vfs::inode::Inode,
        _name: &[u8],
    ) -> Result<alloc::sync::Arc<spin::Mutex<crate::storage::vfs::inode::Inode>>, &'static str>
    {
        Err("not a directory")
    }
    fn create(
        &self,
        _inode: &mut crate::storage::vfs::inode::Inode,
        _name: &[u8],
        _file_type: crate::storage::vfs::inode::FileType,
    ) -> Result<alloc::sync::Arc<spin::Mutex<crate::storage::vfs::inode::Inode>>, &'static str>
    {
        Err("not supported")
    }
    fn unlink(
        &self,
        _inode: &mut crate::storage::vfs::inode::Inode,
        _name: &[u8],
    ) -> Result<(), &'static str> {
        Err("not supported")
    }
    fn mkdir(
        &self,
        _inode: &mut crate::storage::vfs::inode::Inode,
        _name: &[u8],
    ) -> Result<alloc::sync::Arc<spin::Mutex<crate::storage::vfs::inode::Inode>>, &'static str>
    {
        Err("not supported")
    }
    fn rmdir(
        &self,
        _inode: &mut crate::storage::vfs::inode::Inode,
        _name: &[u8],
    ) -> Result<(), &'static str> {
        Err("not supported")
    }
}

pub struct FbFileOps;

impl FileOperations for FbFileOps {
    fn read(
        &self,
        _inode: &crate::storage::vfs::inode::Inode,
        _offset: u64,
        _buf: &mut [u8],
    ) -> Result<usize, &'static str> {
        Ok(0)
    }
    fn write(
        &self,
        _inode: &mut crate::storage::vfs::inode::Inode,
        _offset: u64,
        buf: &[u8],
    ) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn readdir(
        &self,
        _inode: &crate::storage::vfs::inode::Inode,
        _offset: u64,
    ) -> Result<Option<crate::storage::vfs::inode::RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub static FB_INODE_OPS: FbInodeOps = FbInodeOps;
pub static FB_FILE_OPS: FbFileOps = FbFileOps;

pub fn create_fb_file(flags: OpenFlags) -> File {
    let inode = Inode {
        id: 2, // distinc id for fb0
        file_type: FileType::Device,
        inode_ops: &FB_INODE_OPS,
        file_ops: &FB_FILE_OPS,
        size: 0,
        cluster: 0,
        dentry: None,
    };

    File::new(Arc::new(Mutex::new(inode)), flags)
}
