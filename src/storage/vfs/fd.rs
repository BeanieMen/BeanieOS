use alloc::{sync::Arc, vec::Vec};

use spin::Mutex;

use super::file::{File, OpenFlags};
use super::inode::FileType;
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
