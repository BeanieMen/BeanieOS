use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

use crate::storage::vfs::file::{File, OpenFlags};
use crate::storage::vfs::inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry};

pub const TIOCGPTN: u64 = 0x80045430;
pub const TIOCSPTLCK: u64 = 0x40045431;

struct PtyChannel {
    in_buf: VecDeque<u8>,
    out_buf: VecDeque<u8>,
    locked: bool,
}

static PTY_CHANNELS: Mutex<Vec<Option<Arc<Mutex<PtyChannel>>>>> = Mutex::new(Vec::new());

struct PtyInodeOps;

impl InodeOperations for PtyInodeOps {
    fn lookup(&self, _: &Inode, _: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not a directory")
    }

    fn create(
        &self,
        _: &mut Inode,
        _: &[u8],
        _: FileType,
    ) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not supported")
    }

    fn unlink(&self, _: &mut Inode, _: &[u8]) -> Result<(), &'static str> {
        Err("not supported")
    }

    fn mkdir(&self, _: &mut Inode, _: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not supported")
    }

    fn rmdir(&self, _: &mut Inode, _: &[u8]) -> Result<(), &'static str> {
        Err("not supported")
    }
}

static PTY_INODE_OPS: PtyInodeOps = PtyInodeOps;

pub struct PtyMasterOps;

impl FileOperations for PtyMasterOps {
    fn read(&self, inode: &Inode, _offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }
        let id = inode.id as usize;
        let channels = PTY_CHANNELS.lock();
        let Some(Some(chan)) = channels.get(id) else {
            return Err("bad pty");
        };
        let chan = chan.clone();
        drop(channels);

        loop {
            let mut g = chan.lock();
            if !g.out_buf.is_empty() {
                let to_read = buf.len().min(g.out_buf.len());
                let (first, second) = g.out_buf.as_slices();
                let first_len = to_read.min(first.len());
                buf[..first_len].copy_from_slice(&first[..first_len]);
                let second_len = to_read - first_len;
                if second_len > 0 {
                    buf[first_len..to_read].copy_from_slice(&second[..second_len]);
                }
                g.out_buf.drain(..to_read);
                return Ok(to_read);
            }
            drop(g);
            if crate::task::process::has_pending_signals() {
                return Err("interrupted");
            }
            crate::task::scheduler::yield_now();
        }
    }

    fn write(&self, inode: &mut Inode, _offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        let id = inode.id as usize;
        let channels = PTY_CHANNELS.lock();
        let Some(Some(chan)) = channels.get(id) else {
            return Err("bad pty");
        };
        let mut g = chan.lock();
        let to_write = buf.len().min(4096usize.saturating_sub(g.in_buf.len()));
        g.in_buf.extend(buf[..to_write].iter().copied());
        drop(g);
        drop(channels);
        crate::arch::input::wake_waiters();
        Ok(buf.len())
    }

    fn readdir(&self, _inode: &Inode, _offset: u64) -> Result<Option<RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub static PTY_MASTER_OPS: PtyMasterOps = PtyMasterOps;

pub struct PtySlaveOps;

impl FileOperations for PtySlaveOps {
    fn read(&self, inode: &Inode, _offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }
        let id = inode.id as usize;
        let channels = PTY_CHANNELS.lock();
        let Some(Some(chan)) = channels.get(id) else {
            return Err("bad pty");
        };
        let chan = chan.clone();
        drop(channels);

        loop {
            let mut g = chan.lock();
            if !g.in_buf.is_empty() {
                let to_read = buf.len().min(g.in_buf.len());
                let (first, second) = g.in_buf.as_slices();
                let first_len = to_read.min(first.len());
                buf[..first_len].copy_from_slice(&first[..first_len]);
                let second_len = to_read - first_len;
                if second_len > 0 {
                    buf[first_len..to_read].copy_from_slice(&second[..second_len]);
                }
                g.in_buf.drain(..to_read);
                return Ok(to_read);
            }
            drop(g);
            if crate::task::process::has_pending_signals() {
                return Err("interrupted");
            }
            crate::task::scheduler::yield_now();
        }
    }

    fn write(&self, inode: &mut Inode, _offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        let id = inode.id as usize;
        let channels = PTY_CHANNELS.lock();
        let Some(Some(chan)) = channels.get(id) else {
            return Err("bad pty");
        };
        let mut g = chan.lock();
        let to_write = buf.len().min(4096usize.saturating_sub(g.out_buf.len()));
        g.out_buf.extend(buf[..to_write].iter().copied());
        drop(g);
        drop(channels);
        crate::arch::input::wake_waiters();
        Ok(buf.len())
    }

    fn readdir(&self, _inode: &Inode, _offset: u64) -> Result<Option<RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub static PTY_SLAVE_OPS: PtySlaveOps = PtySlaveOps;

pub fn is_master_readable(id: usize) -> bool {
    let channels = PTY_CHANNELS.lock();
    channels
        .get(id)
        .and_then(|c| c.as_ref())
        .map(|c| !c.lock().out_buf.is_empty())
        .unwrap_or(false)
}

pub fn is_master_writable(id: usize) -> bool {
    let channels = PTY_CHANNELS.lock();
    channels
        .get(id)
        .and_then(|c| c.as_ref())
        .map(|c| c.lock().in_buf.len() < 4096)
        .unwrap_or(false)
}

pub fn is_slave_readable(id: usize) -> bool {
    let channels = PTY_CHANNELS.lock();
    channels
        .get(id)
        .and_then(|c| c.as_ref())
        .map(|c| !c.lock().in_buf.is_empty())
        .unwrap_or(false)
}

pub fn is_slave_writable(id: usize) -> bool {
    let channels = PTY_CHANNELS.lock();
    channels
        .get(id)
        .and_then(|c| c.as_ref())
        .map(|c| c.lock().out_buf.len() < 4096)
        .unwrap_or(false)
}

pub fn create_ptmx_file(flags: OpenFlags) -> File {
    let mut channels = PTY_CHANNELS.lock();
    let mut id = None;
    for (i, slot) in channels.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(Arc::new(Mutex::new(PtyChannel {
                in_buf: VecDeque::new(),
                out_buf: VecDeque::new(),
                locked: true,
            })));
            id = Some(i);
            break;
        }
    }
    let pty_id = match id {
        Some(i) => i,
        None => {
            let i = channels.len();
            channels.push(Some(Arc::new(Mutex::new(PtyChannel {
                in_buf: VecDeque::new(),
                out_buf: VecDeque::new(),
                locked: true,
            }))));
            i
        }
    };

    let inode = Inode {
        id: pty_id as u64,
        file_type: FileType::Device,
        size: 0,
        cluster: 0,
        inode_ops: &PTY_INODE_OPS,
        file_ops: &PTY_MASTER_OPS,
        dentry: None,
        parent_cluster: 0,
        dir_name: [0u8; 11],
    };
    File::new(Arc::new(Mutex::new(inode)), flags)
}

pub fn create_pts_file(id: usize, flags: OpenFlags) -> Result<File, &'static str> {
    let channels = PTY_CHANNELS.lock();
    let Some(Some(chan)) = channels.get(id) else {
        return Err("pty not found");
    };
    if chan.lock().locked {
        return Err("pty locked");
    }

    let inode = Inode {
        id: id as u64,
        file_type: FileType::Device,
        size: 0,
        cluster: 0,
        inode_ops: &PTY_INODE_OPS,
        file_ops: &PTY_SLAVE_OPS,
        dentry: None,
        parent_cluster: 0,
        dir_name: [0u8; 11],
    };
    Ok(File::new(Arc::new(Mutex::new(inode)), flags))
}

pub fn unlock_pty(id: usize) -> bool {
    let channels = PTY_CHANNELS.lock();
    if let Some(Some(chan)) = channels.get(id) {
        chan.lock().locked = false;
        true
    } else {
        false
    }
}