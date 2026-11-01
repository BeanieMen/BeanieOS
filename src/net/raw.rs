//! Raw Ethernet frame sockets (AF_PACKET-style), the mechanism userspace
//! network clients (DHCP, link-local protocols) build on. The kernel taps a
//! copy of every received frame into each open raw socket and transmits
//! frames written to one; it applies no addressing or configuration policy
//! itself - that lives in userspace.

use alloc::{collections::BTreeMap, collections::VecDeque, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
use spin::Mutex;

use crate::storage::vfs::{
    file::{File, OpenFlags},
    inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry},
};
use crate::task::scheduler::yield_now;

/// Frames buffered per raw socket before the oldest are dropped.
const MAX_QUEUED_FRAMES: usize = 64;
/// Ethernet header plus the 1500-byte MTU.
const MAX_FRAME_LEN: usize = 1514;

pub struct RawSocket {
    pub rx_queue: VecDeque<Vec<u8>>,
}

pub static RAW_SOCKETS: Mutex<Vec<alloc::sync::Weak<Mutex<RawSocket>>>> =
    Mutex::new(Vec::new());
pub static RAW_MAP: Mutex<BTreeMap<u64, Arc<Mutex<RawSocket>>>> = Mutex::new(BTreeMap::new());
static NEXT_RAW_ID: AtomicU64 = AtomicU64::new(0x4_0000_0000);

/// Copy a received frame into every open raw socket. Called from the device
/// receive path before the frame is handed to the protocol stack.
pub fn tap_frame(frame: &[u8]) {
    let mut sockets = RAW_SOCKETS.lock();
    // Weak refs let closed sockets disappear instead of buffering forever.
    sockets.retain(|entry| entry.upgrade().is_some());
    for entry in sockets.iter() {
        let Some(sock) = entry.upgrade() else {
            continue;
        };
        let mut sock = sock.lock();
        if sock.rx_queue.len() >= MAX_QUEUED_FRAMES {
            sock.rx_queue.pop_front();
        }
        sock.rx_queue.push_back(frame.to_vec());
    }
}

/// Nonblocking-read helper: whether this raw socket has nothing queued.
pub fn queue_empty(inode_id: u64) -> bool {
    let map = RAW_MAP.lock();
    match map.get(&inode_id) {
        Some(sock) => sock.lock().rx_queue.is_empty(),
        None => true,
    }
}

pub struct RawInodeOps;
impl InodeOperations for RawInodeOps {
    fn lookup(&self, _inode: &Inode, _name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not a directory")
    }
    fn create(
        &self,
        _inode: &mut Inode,
        _name: &[u8],
        _file_type: FileType,
    ) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not supported")
    }
    fn unlink(&self, _inode: &mut Inode, _name: &[u8]) -> Result<(), &'static str> {
        Err("not supported")
    }
    fn mkdir(
        &self,
        _inode: &mut Inode,
        _name: &[u8],
    ) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not supported")
    }
    fn rmdir(&self, _inode: &mut Inode, _name: &[u8]) -> Result<(), &'static str> {
        Err("not supported")
    }
}

pub struct RawFileOps;
pub static RAW_INODE_OPS: RawInodeOps = RawInodeOps;
pub static RAW_FILE_OPS: RawFileOps = RawFileOps;

impl FileOperations for RawFileOps {
    fn read(&self, inode: &Inode, _offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        let sock = {
            let map = RAW_MAP.lock();
            map.get(&inode.id).cloned().ok_or("socket not found")?
        };
        loop {
            crate::net::stack::poll();
            {
                let mut s = sock.lock();
                if let Some(frame) = s.rx_queue.pop_front() {
                    let n = core::cmp::min(buf.len(), frame.len());
                    buf[..n].copy_from_slice(&frame[..n]);
                    return Ok(n);
                }
            }
            if crate::task::process::has_pending_signals() {
                return Err("interrupted");
            }
            yield_now();
        }
    }

    fn write(&self, _inode: &mut Inode, _offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        if buf.len() < 14 || buf.len() > MAX_FRAME_LEN {
            return Err("invalid frame length");
        }
        let dev = super::e1000::E1000_DEVICE.lock();
        let d = dev.as_ref().ok_or("no network device")?;
        if !d.transmit(buf) {
            return Err("transmit queue full");
        }
        Ok(buf.len())
    }

    fn readdir(&self, _inode: &Inode, _offset: u64) -> Result<Option<RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub fn create_raw_socket_file() -> File {
    let sock = Arc::new(Mutex::new(RawSocket {
        rx_queue: VecDeque::new(),
    }));
    let id = NEXT_RAW_ID.fetch_add(1, Relaxed);
    RAW_MAP.lock().insert(id, sock.clone());
    RAW_SOCKETS.lock().push(Arc::downgrade(&sock));

    let inode = Inode {
        id,
        file_type: FileType::Device,
        size: 0,
        cluster: 0,
        inode_ops: &RAW_INODE_OPS,
        file_ops: &RAW_FILE_OPS,
        dentry: None,
        parent_cluster: 0,
        dir_name: [0u8; 11],
    };
    File::new(Arc::new(Mutex::new(inode)), OpenFlags::RDWR)
}
