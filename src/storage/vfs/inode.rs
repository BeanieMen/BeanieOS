use alloc::sync::Arc;

use spin::Mutex;

use super::dentry::Dentry;

type InodeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileType {
    Regular,
    Directory,
    Symlink,
    Device,
}

pub(crate) struct Inode {
    pub id: InodeId,
    pub file_type: FileType,
    pub size: u64,
    pub cluster: u32,
    pub inode_ops: &'static dyn InodeOperations,
    pub file_ops: &'static dyn FileOperations,

    pub dentry: Option<Arc<Mutex<Dentry>>>,
}

pub(crate) trait InodeOperations: Sync {
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

pub(crate) struct RawEntry {
    pub inode: u64,
    pub file_type: FileType,
    pub size: u32,
    pub name: [u8; 11],
}

pub(crate) trait FileOperations: Sync {
    fn read(&self, inode: &Inode, offset: u64, buf: &mut [u8]) -> Result<usize, &'static str>;

    fn write(&self, inode: &mut Inode, offset: u64, buf: &[u8]) -> Result<usize, &'static str>;

    fn readdir(&self, inode: &Inode, offset: u64) -> Result<Option<RawEntry>, &'static str>;
}
