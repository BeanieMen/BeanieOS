use alloc::{sync::Arc, vec::Vec};

use spin::Mutex;

use super::file::{File, OpenFlags};
use super::inode::{FileType, Inode};
use super::mount::{FileSystem, MountTable};

pub(crate) struct FdTable {
    slots: Vec<Option<File>>,
    mounts: MountTable,
}

impl FdTable {
    pub(crate) fn new() -> Self {
        FdTable {
            slots: Vec::new(),
            mounts: MountTable::new(),
        }
    }

    pub(crate) fn mount(&mut self, fs: Arc<dyn FileSystem>) {
        self.mounts.mount(fs, None);
    }

    pub(crate) fn mount_count(&self) -> usize {
        self.mounts.len()
    }

    pub(crate) fn sync_all(&self) -> Result<(), &'static str> {
        self.mounts.sync_all()
    }

    pub(crate) fn alloc(&mut self, file: File) -> u64 {
        if let Some(fd) = self.slots.iter().position(|slot| slot.is_none()) {
            self.slots[fd] = Some(file);
            return fd as u64;
        }

        self.slots.push(Some(file));

        (self.slots.len() - 1) as u64
    }

    pub(crate) fn get(&self, fd: u64) -> Result<&File, &'static str> {
        self.slots
            .get(fd as usize)
            .and_then(|slot| slot.as_ref())
            .ok_or("bad file descriptor")
    }

    fn get_mut(&mut self, fd: u64) -> Result<&mut File, &'static str> {
        self.slots
            .get_mut(fd as usize)
            .and_then(|slot| slot.as_mut())
            .ok_or("bad file descriptor")
    }

    pub(crate) fn close(&mut self, fd: u64) -> Result<(), &'static str> {
        let slot = self
            .slots
            .get_mut(fd as usize)
            .ok_or("bad file descriptor")?;

        if slot.is_none() {
            return Err("bad file descriptor");
        }

        *slot = None;

        Ok(())
    }

    pub(crate) fn read(&mut self, fd: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        self.get_mut(fd)?.read(buf)
    }

    pub(crate) fn write(&mut self, fd: u64, buf: &[u8]) -> Result<usize, &'static str> {
        self.get_mut(fd)?.write(buf)
    }

    pub(crate) fn getdents(&mut self, fd: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        self.get_mut(fd)?.getdents(buf)
    }

    pub(crate) fn open(&mut self, path: &[u8], flags: OpenFlags) -> Result<u64, &'static str> {
        let resolved = self.mounts.resolve(path)?;

        let inode = resolved.inode();

        {
            let inode = inode.lock();

            if inode.file_type != FileType::Regular {
                return Err("not a regular file");
            }
        }

        Ok(self.alloc(File::new(inode, flags)))
    }

    pub(crate) fn open_read(&mut self, path: &[u8]) -> Result<u64, &'static str> {
        self.open(path, OpenFlags::READ)
    }

    pub(crate) fn mkdir(&mut self, path: &[u8]) -> Result<(), &'static str> {
        let (parent, name) = self.parent_of(path)?;

        let mut parent_inode = parent.lock();

        parent_inode.inode_ops.mkdir(&mut parent_inode, &name)?;

        Ok(())
    }

    pub(crate) fn rmdir(&mut self, path: &[u8]) -> Result<(), &'static str> {
        let (parent, name) = self.parent_of(path)?;

        let mut parent_inode = parent.lock();

        parent_inode.inode_ops.rmdir(&mut parent_inode, &name)?;

        Ok(())
    }

    pub(crate) fn unlink(&mut self, path: &[u8]) -> Result<(), &'static str> {
        let (parent, name) = self.parent_of(path)?;

        let mut parent_inode = parent.lock();

        parent_inode.inode_ops.unlink(&mut parent_inode, &name)?;

        Ok(())
    }

    /// Creates an empty regular file, or returns the handle if it is already there.
    pub(crate) fn touch(&mut self, path: &[u8]) -> Result<(), &'static str> {
        let (parent, name) = self.parent_of(path)?;

        let mut parent_inode = parent.lock();

        if parent_inode.inode_ops.lookup(&parent_inode, &name).is_ok() {
            return Ok(());
        }

        parent_inode
            .inode_ops
            .create(&mut parent_inode, &name, FileType::Regular)?;

        Ok(())
    }

    /// Splits a path into the parent's inode and the final component.
    ///
    /// Done on the text rather than on a resolved dentry, because the root of a
    /// filesystem has no parent to walk back to and yet still has to be
    /// writable.
    fn parent_of(&self, path: &[u8]) -> Result<(Arc<Mutex<Inode>>, Vec<u8>), &'static str> {
        let mut end = path.len();

        while end > 1 && path[end - 1] == b'/' {
            end -= 1;
        }

        let mut split = end;

        while split > 0 && path[split - 1] != b'/' {
            split -= 1;
        }

        let name = path[split..end].to_vec();

        if name.is_empty() {
            return Err("no file name");
        }

        let parent = if split <= 1 {
            b"/".as_slice()
        } else {
            &path[..split - 1]
        };

        let resolved = self.mounts.resolve(parent)?;

        Ok((resolved.inode(), name))
    }

    pub(crate) fn open_dir(&mut self, path: &[u8]) -> Result<u64, &'static str> {
        let resolved = self.mounts.resolve(path)?;

        let inode = resolved.inode();

        {
            let inode = inode.lock();

            if inode.file_type != FileType::Directory {
                return Err("not a directory");
            }
        }

        Ok(self.alloc(File::new(inode, OpenFlags::READ)))
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn close_all(&mut self) {
        self.slots.clear();
    }
}
