use alloc::sync::Arc;

use spin::Mutex;

use super::dentry::Dentry;

pub type InodeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    Device,
}

pub struct Inode {
    pub id: InodeId,
    pub file_type: FileType,
    pub size: u64,
    pub cluster: u32,
    pub inode_ops: &'static dyn InodeOperations,
    pub file_ops: &'static dyn FileOperations,

    pub dentry: Option<Arc<Mutex<Dentry>>>,
}

pub trait InodeOperations: Sync {
    fn lookup(&self, inode: &Inode, name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str>;

    fn create(
        &self,
        inode: &mut Inode,
        name: &[u8],
        file_type: FileType,
    ) -> Result<Arc<Mutex<Inode>>, &'static str>;

    fn unlink(&self, inode: &mut Inode, name: &[u8]) -> Result<(), &'static str>;

    fn mkdir(&self, inode: &mut Inode, name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str>;

    fn rmdir(&self, inode: &mut Inode, name: &[u8]) -> Result<(), &'static str>;
}

pub trait FileOperations: Sync {
    fn read(&self, inode: &Inode, offset: u64, buf: &mut [u8]) -> Result<usize, &'static str>;

    fn write(&self, inode: &mut Inode, offset: u64, buf: &[u8]) -> Result<usize, &'static str>;
}
