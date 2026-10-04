use alloc::{sync::Arc, vec::Vec};

use spin::Mutex;

use super::dentry::Dentry;
use super::file::{File, OpenFlags};
use super::inode::FileType;
use super::path::Path;

pub struct FdTable {
    slots: Vec<Option<File>>,
    root: Option<Arc<Mutex<Dentry>>>,
}

impl FdTable {
    pub fn new() -> Self {
        FdTable {
            slots: Vec::new(),
            root: None,
        }
    }

    pub fn set_root(&mut self, root: Arc<Mutex<Dentry>>) {
        self.root = Some(root);
    }

    pub fn alloc(&mut self, file: File) -> u64 {
        if let Some(fd) = self.slots.iter().position(|slot| slot.is_none()) {
            self.slots[fd] = Some(file);
            return fd as u64;
        }

        self.slots.push(Some(file));

        (self.slots.len() - 1) as u64
    }

    pub fn get(&self, fd: u64) -> Result<&File, &'static str> {
        self.slots
            .get(fd as usize)
            .and_then(|slot| slot.as_ref())
            .ok_or("bad file descriptor")
    }

    pub fn get_mut(&mut self, fd: u64) -> Result<&mut File, &'static str> {
        self.slots
            .get_mut(fd as usize)
            .and_then(|slot| slot.as_mut())
            .ok_or("bad file descriptor")
    }

    pub fn close(&mut self, fd: u64) -> Result<(), &'static str> {
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

    pub fn read(&mut self, fd: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        self.get_mut(fd)?.read(buf)
    }

    pub fn write(&mut self, fd: u64, buf: &[u8]) -> Result<usize, &'static str> {
        self.get_mut(fd)?.write(buf)
    }

    pub fn open(&mut self, path: &[u8], flags: OpenFlags) -> Result<u64, &'static str> {
        let root = self.root.clone().ok_or("no filesystem mounted")?;

        let resolved = Path::resolve(root, path)?;

        let inode = resolved.inode();

        {
            let inode = inode.lock();

            if inode.file_type != FileType::Regular {
                return Err("not a regular file");
            }
        }

        Ok(self.alloc(File::new(inode, flags)))
    }

    pub fn open_read(&mut self, path: &[u8]) -> Result<u64, &'static str> {
        self.open(path, OpenFlags::READ)
    }

    pub fn len(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn close_all(&mut self) {
        self.slots.clear();
    }
}
