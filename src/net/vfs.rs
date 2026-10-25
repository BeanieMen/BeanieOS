use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec::Vec,
};
use spin::mutex::Mutex;

use crate::{
    net::unix::{SocketState, UnixSocket},
    storage::vfs::{
        file::{File, OpenFlags},
        inode::{FileOperations, FileType::Device, Inode, InodeOperations, RawEntry},
    },
    task::scheduler::yield_now,
};

pub struct SocketInodeOps;
impl InodeOperations for SocketInodeOps {
    fn lookup(
        &self,
        _inode: &Inode,
        _name: &[u8],
    ) -> Result<Arc<spin::Mutex<Inode>>, &'static str> {
        Err("not a directory")
    }

    fn create(
        &self,
        _inode: &mut Inode,
        _name: &[u8],
        _file_type: crate::storage::vfs::inode::FileType,
    ) -> Result<Arc<spin::Mutex<Inode>>, &'static str> {
        Err("not supported")
    }

    fn unlink(
        &self,
        _inode: &mut Inode,
        _name: &[u8],
    ) -> Result<(), &'static str> {
        Err("not supported")
    }

    fn mkdir(
        &self,
        _inode: &mut Inode,
        _name: &[u8],
    ) -> Result<Arc<spin::Mutex<Inode>>, &'static str> {
        Err("not supported")
    }

    fn rmdir(
        &self,
        _inode: &mut Inode,
        _name: &[u8],
    ) -> Result<(), &'static str> {
        Err("not supported")
    }
}

pub struct SocketFileOps;

impl FileOperations for SocketFileOps {
    fn read(
        &self,
        inode: &Inode,
        _offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, &'static str> {
        let sock = {
            let map = SOCKET_MAP.lock();
            map.get(&inode.id).cloned().ok_or("socket not found")?
        };

        loop {
            let mut s = sock.lock();
            match &mut s.state {
                SocketState::Connected {
                    peer: _,
                    rx_buffer,
                    peer_closed,
                } => {
                    if !rx_buffer.is_empty() {
                        let to_read = core::cmp::min(buf.len(), rx_buffer.len());

                        for i in 0..to_read {
                            buf[i] = rx_buffer.pop_front().unwrap();
                        }
                        return Ok(to_read);
                    }

                    if *peer_closed {
                        return Ok(0);
                    }
                }

                SocketState::Closed => return Ok(0),
                _ => return Err("socket not connected"),
            }
            drop(s);
            if crate::task::process::has_pending_signals() {
                return Err("interrupted");
            }
            yield_now();
        }
    }

    fn write(
        &self,
        inode: &mut Inode,
        _offset: u64,
        buf: &[u8],
    ) -> Result<usize, &'static str> {
        let sock = {
            let map = SOCKET_MAP.lock();
            map.get(&inode.id).cloned().ok_or("socket not found")?
        };
        let peer_arc = {
            let s = sock.lock();
            match &s.state {
                SocketState::Connected {
                    peer, peer_closed, ..
                } => {
                    if *peer_closed {
                        return Err("broken pipe");
                    }
                    peer.upgrade().ok_or("broken pipe")?
                }
                _ => return Err("socket not connected"),
            }
        };
        let mut peer_sock = peer_arc.lock();
        if let SocketState::Connected { rx_buffer, .. } = &mut peer_sock.state {
            for &b in buf {
                if rx_buffer.len() < 65536 {
                    rx_buffer.push_back(b);
                } else {
                    break;
                }
            }
            Ok(buf.len())
        } else {
            Err("broken pipe")
        }
    }

    fn readdir(
        &self,
        _inode: &Inode,
        _offset: u64,
    ) -> Result<Option<RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub static SOCKET_MAP: Mutex<BTreeMap<u64, Arc<Mutex<UnixSocket>>>> = Mutex::new(BTreeMap::new());
static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(1000);

pub static SOCKET_REGISTRY: Mutex<BTreeMap<Vec<u8>, Arc<Mutex<UnixSocket>>>> =
    Mutex::new(BTreeMap::new());

pub static SOCKET_INODE_OPS: SocketInodeOps = SocketInodeOps;
pub static SOCKET_FILE_OPS: SocketFileOps = SocketFileOps;

pub fn create_socket_file(sock: Arc<Mutex<UnixSocket>>) -> File {
    let id = NEXT_SOCKET_ID.fetch_add(1, Relaxed);

    SOCKET_MAP.lock().insert(id, sock);

    let inode = Inode {
        id,
        file_type: Device,
        size: 0,
        cluster: 0,
        inode_ops: &SOCKET_INODE_OPS,
        file_ops: &SOCKET_FILE_OPS,
        dentry: None,
    };

    File::new(Arc::new(Mutex::new(inode)), OpenFlags::RDWR)
}

pub fn create_connected_pair() -> (Arc<Mutex<UnixSocket>>, Arc<Mutex<UnixSocket>>) {
    let a = Arc::new(Mutex::new(UnixSocket::new()));
    let b = Arc::new(Mutex::new(UnixSocket::new()));
    a.lock().state = SocketState::Connected {
        peer: Arc::downgrade(&b),
        rx_buffer: VecDeque::new(),
        peer_closed: false,
    };
    b.lock().state = SocketState::Connected {
        peer: Arc::downgrade(&a),
        rx_buffer: VecDeque::new(),
        peer_closed: false,
    };
    (a, b)
}
