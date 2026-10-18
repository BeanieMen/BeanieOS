use alloc::sync::Arc;
use spin::Mutex;

use crate::storage::vfs::{file::{File, OpenFlags}, inode::{FileOperations, FileType, Inode, InodeOperations}};

pub struct FbInodeOps;
impl InodeOperations for FbInodeOps {
    fn lookup(
        &self,
        inode: &crate::storage::vfs::inode::Inode,
        name: &[u8],
    ) -> Result<alloc::sync::Arc<spin::Mutex<crate::storage::vfs::inode::Inode>>, &'static str>
    {
        Err("not a directory")
    }
    fn create(
        &self,
        inode: &mut crate::storage::vfs::inode::Inode,
        name: &[u8],
        file_type: crate::storage::vfs::inode::FileType,
    ) -> Result<alloc::sync::Arc<spin::Mutex<crate::storage::vfs::inode::Inode>>, &'static str>
    {
        Err("not supported")
    }
    fn unlink(
        &self,
        inode: &mut crate::storage::vfs::inode::Inode,
        name: &[u8],
    ) -> Result<(), &'static str> {
        Err("not supported")
    }
    fn mkdir(
        &self,
        inode: &mut crate::storage::vfs::inode::Inode,
        name: &[u8],
    ) -> Result<alloc::sync::Arc<spin::Mutex<crate::storage::vfs::inode::Inode>>, &'static str>
    {
        Err("not supported")
    }
    fn rmdir(
        &self,
        inode: &mut crate::storage::vfs::inode::Inode,
        name: &[u8],
    ) -> Result<(), &'static str> {
        Err("not supported")
    }
}

pub struct FbFileOps;

impl FileOperations for FbFileOps {
    fn read(
        &self,
        inode: &crate::storage::vfs::inode::Inode,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, &'static str> {
        Ok(0)
    }
    fn write(
        &self,
        inode: &mut crate::storage::vfs::inode::Inode,
        offset: u64,
        buf: &[u8],
    ) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn readdir(
        &self,
        inode: &crate::storage::vfs::inode::Inode,
        offset: u64,
    ) -> Result<Option<crate::storage::vfs::inode::RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub static FB_INODE_OPS: FbInodeOps = FbInodeOps;
pub static FB_FILE_OPS: FbFileOps = FbFileOps;

pub fn create_fb_file(flags: OpenFlags) -> File{
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
