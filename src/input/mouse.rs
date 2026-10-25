use alloc::collections::VecDeque;
use alloc::sync::Arc;
use spin::Mutex;

use crate::arch::lock::InterruptMutex;
use crate::storage::vfs::file::{File, OpenFlags};
use crate::storage::vfs::inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry};

pub static MOUSE_QUEUE: InterruptMutex<VecDeque<u8>> = InterruptMutex::new(VecDeque::new());

/// Push a standard 3-byte PS/2 mouse packet into the queue.
pub fn push_packet(flags: u8, x: u8, y: u8) {
    let mut q = MOUSE_QUEUE.lock();
    // Maintain at most 1024 bytes (approx 341 packets) to prevent stale lag.
    // Drop complete 3-byte packets so packet framing is never corrupted.
    while q.len() + 3 > 1024 {
        q.pop_front();
        q.pop_front();
        q.pop_front();
    }
    q.push_back(flags);
    q.push_back(x);
    q.push_back(y);
    drop(q);
    crate::arch::input::wake_waiters();
}

pub struct MouseInodeOps;

impl InodeOperations for MouseInodeOps {
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

pub struct MouseFileOps;

impl FileOperations for MouseFileOps {
    fn read(&self, _inode: &Inode, _offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut count = 0;
        while count == 0 {
            {
                let mut q = MOUSE_QUEUE.lock();
                while count < buf.len() {
                    if let Some(b) = q.pop_front() {
                        buf[count] = b;
                        count += 1;
                    } else {
                        break;
                    }
                }
            }
            if count > 0 {
                return Ok(count);
            }
            if crate::task::process::has_pending_signals() {
                return Err("interrupted");
            }
            crate::syscall::dispatch::wait_for_key();
        }
        Ok(count)
    }

    fn write(&self, _inode: &mut Inode, _offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        let mut q = MOUSE_QUEUE.lock();
        for &byte in buf {
            match byte {
                0xF2 => {
                    // Get device ID -> ACK + standard PS/2 mouse ID 0x00
                    q.push_back(0xFA);
                    q.push_back(0x00);
                }
                0xFF => {
                    // Reset -> ACK + BAT test passed (0xAA) + ID (0x00)
                    q.push_back(0xFA);
                    q.push_back(0xAA);
                    q.push_back(0x00);
                }
                0xE9 => {
                    // Status request -> ACK + status bytes
                    q.push_back(0xFA);
                    q.push_back(0x00);
                    q.push_back(0x02);
                    q.push_back(0x64);
                }
                _ => {
                    // Standard PS/2 ACK for set sample rate (0xF3), resolution (0xE8), scaling, etc.
                    q.push_back(0xFA);
                }
            }
        }
        drop(q);
        crate::arch::input::wake_waiters();
        Ok(buf.len())
    }

    fn readdir(&self, _inode: &Inode, _offset: u64) -> Result<Option<RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub static MOUSE_INODE_OPS: MouseInodeOps = MouseInodeOps;
pub static MOUSE_FILE_OPS: MouseFileOps = MouseFileOps;

pub fn create_mouse_file(flags: OpenFlags) -> File {
    let inode = Inode {
        id: 8,
        file_type: FileType::Device,
        size: 0,
        cluster: 0,
        inode_ops: &MOUSE_INODE_OPS,
        file_ops: &MOUSE_FILE_OPS,
        dentry: None,
        parent_cluster: 0,
        dir_name: [0u8; 11],
    };
    File::new(Arc::new(Mutex::new(inode)), flags)
}
