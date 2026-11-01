use alloc::sync::{Arc, Weak};

use spin::Mutex;

use super::dentry::Dentry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileType {
    Regular,
    Directory,
    Symlink,
    Device,
}

#[derive(Clone)]
pub(crate) struct Inode {
    pub id: u64,
    pub file_type: FileType,
    pub size: u64,
    pub cluster: u32,
    pub inode_ops: &'static dyn InodeOperations,
    pub file_ops: &'static dyn FileOperations,

    pub dentry: Option<Weak<Mutex<Dentry>>>,
    pub parent_cluster: u32,
    pub dir_name: [u8; 11],
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

    /// Move `old_name` from `old_dir` to `new_name` in `new_dir`, replacing
    /// any existing destination with the same shape (file over file, empty
    /// directory over empty directory). Both parents are on this filesystem;
    /// the VFS rejects cross-filesystem renames before calling. On success,
    /// returns the identity needed to keep open handles pointing at their
    /// file's current directory entry.
    fn rename(
        &self,
        _old_dir: &Inode,
        _old_name: &[u8],
        _new_dir: &Inode,
        _new_name: &[u8],
    ) -> Result<Option<RenameNote>, &'static str> {
        Err("rename not supported")
    }
}

/// Where a renamed file's directory entry lives now. The syscall layer
/// applies this to every open Inode so later metadata writes (like size
/// updates) land in the renamed entry, not a stale location.
#[derive(Clone, Copy)]
pub(crate) struct RenameNote {
    pub cluster: u32,
    pub old_parent: u32,
    pub old_name: [u8; 11],
    pub new_parent: u32,
    pub new_name: [u8; 11],
}

pub(crate) struct RawEntry {
    pub inode: u64,
    pub file_type: FileType,
    pub size: u32,
    pub name: [u8; 256],
    pub name_len: usize,
}

pub(crate) trait FileOperations: Sync {
    fn read(&self, inode: &Inode, offset: u64, buf: &mut [u8]) -> Result<usize, &'static str>;

    fn write(&self, inode: &mut Inode, offset: u64, buf: &[u8]) -> Result<usize, &'static str>;

    fn readdir(&self, inode: &Inode, offset: u64) -> Result<Option<RawEntry>, &'static str>;

    /// Make this file's prior writes durable. Implementations without a
    /// backing device have nothing to flush.
    fn flush(&self, _inode: &Inode) -> Result<(), &'static str> {
        Ok(())
    }
}
