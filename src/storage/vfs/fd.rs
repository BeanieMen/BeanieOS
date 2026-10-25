use alloc::{sync::Arc, vec::Vec};

use spin::Mutex;

use super::file::{File, OpenFlags};
use super::inode::{FileType, Inode};
use super::mount::{FileSystem, MountTable};

#[derive(Clone)]
pub(crate) struct FdTable {
    slots: Vec<Option<File>>,
    mounts: MountTable,
}

impl FdTable {
    pub(crate) fn new() -> Self {
        let slots = alloc::vec![
            Some(crate::tty::create_tty_file(OpenFlags::READ)),
            Some(crate::tty::create_tty_file(OpenFlags::WRITE)),
            Some(crate::tty::create_tty_file(OpenFlags::WRITE)),
        ];
        FdTable {
            slots,
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
        self.alloc_at_least(file, 0)
    }

    pub(crate) fn alloc_at_least(&mut self, file: File, min_fd: usize) -> u64 {
        while self.slots.len() <= min_fd {
            self.slots.push(None);
        }

        if let Some(fd) = self
            .slots
            .iter()
            .enumerate()
            .position(|(i, slot)| i >= min_fd && slot.is_none())
        {
            self.slots[fd] = Some(file);
            return fd as u64;
        }

        self.slots.push(Some(file));

        (self.slots.len() - 1) as u64
    }

    pub(crate) fn dup2(&mut self, oldfd: u64, newfd: u64) -> Result<u64, &'static str> {
        let file = self.get(oldfd)?.clone();
        if oldfd == newfd {
            return Ok(newfd);
        }
        let target = newfd as usize;
        while self.slots.len() <= target {
            self.slots.push(None);
        }
        self.slots[target] = Some(file);
        Ok(newfd)
    }

    pub(crate) fn get(&self, fd: u64) -> Result<&File, &'static str> {
        self.slots
            .get(fd as usize)
            .and_then(|slot| slot.as_ref())
            .ok_or("bad file descriptor")
    }

    pub(crate) fn get_mut(&mut self, fd: u64) -> Result<&mut File, &'static str> {
        self.slots
            .get_mut(fd as usize)
            .and_then(|slot| slot.as_mut())
            .ok_or("bad file descriptor")
    }

    pub(crate) fn lseek(&mut self, fd: u64, offset: i64, whence: u32) -> Result<u64, &'static str> {
        let file = self.get_mut(fd)?;

        let base = match whence {
            0 => 0i64,
            1 => file.tell() as i64,
            2 => file.size() as i64,
            _ => return Err("bad whence"),
        };

        let target = base.checked_add(offset).ok_or("seek overflow")?;

        if target < 0 {
            return Err("seek before start of file");
        }

        file.seek(target as u64)
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
        if path == b"/dev/fb0" || path == b"dev/fb0" {
            let fb_file = crate::graphics::vfs::create_fb_file(flags);
            return Ok(self.alloc(fb_file));
        }

        if path == b"/dev/dsp" || path == b"dev/dsp" {
            let dsp_file = crate::audio::vfs::create_dsp_file(flags);
            return Ok(self.alloc(dsp_file));
        }

        if path == b"/dev/input/event0" || path == b"dev/input/event0" || path == b"/dev/event0" {
            let event_file = crate::input::event::create_event_file(flags);
            return Ok(self.alloc(event_file));
        }

        if path == b"/dev/mouse" || path == b"dev/mouse"
            || path == b"/dev/psaux" || path == b"dev/psaux"
            || path == b"/dev/input/mice" || path == b"dev/input/mice"
            || path == b"/dev/input/mouse0" || path == b"dev/input/mouse0"
        {
            let mouse_file = crate::input::mouse::create_mouse_file(flags);
            return Ok(self.alloc(mouse_file));
        }

        if path == b"/dev/tty1" || path == b"dev/tty1" || path == b"/dev/tty0" || path == b"dev/tty0" || path == b"/dev/tty" || path == b"dev/tty" || path == b"/dev/console" {
            let tty_file = crate::tty::vfs::create_tty_file(flags);
            return Ok(self.alloc(tty_file));
        }

        if path.starts_with(b"/dev/tty") || path.starts_with(b"dev/tty") {
            let vt2_file = crate::tty::vfs::create_vt2_file(flags);
            return Ok(self.alloc(vt2_file));
        }

        
        let resolved = match self.mounts.resolve(path) {
            Ok(dentry) => {
                if flags.create && flags.exclusive {
                    return Err("file already exists");
                }
                dentry
            }
            Err(_) if flags.create => {
                self.touch(path)?;
                self.mounts.resolve(path)?
            }
            Err(e) => return Err(e),
        };

        let inode = resolved.inode();

        {
            let mut inode_guard = inode.lock();

            if flags.directory && inode_guard.file_type != FileType::Directory {
                return Err("not a directory");
            }

            if inode_guard.file_type == FileType::Directory {
                if flags.write {
                    return Err("is a directory");
                }
            } else if inode_guard.file_type != FileType::Regular {
                return Err("not a regular file");
            }

            if flags.truncate && inode_guard.file_type == FileType::Regular {
                inode_guard.size = 0;
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
        crate::storage::vfs::path::invalidate_resolve_cache();

        Ok(())
    }

    pub(crate) fn rmdir(&mut self, path: &[u8]) -> Result<(), &'static str> {
        let (parent, name) = self.parent_of(path)?;

        let mut parent_inode = parent.lock();

        parent_inode.inode_ops.rmdir(&mut parent_inode, &name)?;
        crate::storage::vfs::path::invalidate_resolve_cache();

        Ok(())
    }

    pub(crate) fn unlink(&mut self, path: &[u8]) -> Result<(), &'static str> {
        let (parent, name) = self.parent_of(path)?;

        let mut parent_inode = parent.lock();

        parent_inode.inode_ops.unlink(&mut parent_inode, &name)?;
        crate::storage::vfs::path::invalidate_resolve_cache();

        Ok(())
    }

    pub(crate) fn touch(&mut self, path: &[u8]) -> Result<(), &'static str> {
        let (parent, name) = self.parent_of(path)?;

        let mut parent_inode = parent.lock();

        if parent_inode.inode_ops.lookup(&parent_inode, &name).is_ok() {
            return Ok(());
        }

        parent_inode
            .inode_ops
            .create(&mut parent_inode, &name, FileType::Regular)?;
        crate::storage::vfs::path::invalidate_resolve_cache();

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
        self.open(path, OpenFlags::READ_DIR)
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn close_all(&mut self) {
        self.slots.clear();
    }
}
