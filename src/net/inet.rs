use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering::Relaxed};
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp::{Socket as TcpSocket, State as TcpState};
use smoltcp::socket::udp::{PacketBuffer, Socket as UdpSocket};
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address};
use spin::Mutex;

use crate::storage::vfs::{
    file::{File, OpenFlags},
    inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry},
};
use crate::task::scheduler::yield_now;

pub static NEXT_INET_INODE: AtomicU64 = AtomicU64::new(0x8000_0000);
pub static INET_SOCKET_MAP: Mutex<alloc::collections::BTreeMap<u64, Arc<Mutex<InetSocket>>>> =
    Mutex::new(alloc::collections::BTreeMap::new());

pub enum InetType {
    Tcp,
    Udp,
}

pub struct InetSocket {
    pub sock_type: InetType,
    pub handle: Option<SocketHandle>,
    pub bound_port: u16,
    pub remote_endpoint: Option<IpEndpoint>,
}

impl InetSocket {
    pub fn new(sock_type: InetType) -> Self {
        InetSocket {
            sock_type,
            handle: None,
            bound_port: 0,
            remote_endpoint: None,
        }
    }
}

pub struct InetInodeOps;
impl InodeOperations for InetInodeOps {
    fn lookup(&self, _inode: &Inode, _name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not supported")
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
    fn mkdir(&self, _inode: &mut Inode, _name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("not supported")
    }
    fn rmdir(&self, _inode: &mut Inode, _name: &[u8]) -> Result<(), &'static str> {
        Err("not supported")
    }
}

pub struct InetFileOps;
pub static INET_INODE_OPS: InetInodeOps = InetInodeOps;
pub static INET_FILE_OPS: InetFileOps = InetFileOps;

impl FileOperations for InetFileOps {
    fn read(&self, inode: &Inode, _offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        let sock = {
            let map = INET_SOCKET_MAP.lock();
            map.get(&inode.id).cloned().ok_or("socket not found")?
        };

        loop {
            crate::net::stack::poll();
            {
                let s = sock.lock();
                if let Some(handle) = s.handle {
                    let mut guard = crate::net::stack::NET_STATE.lock();
                    if let Some(state) = guard.as_mut() {
                        let socket = state.sockets.get_mut::<TcpSocket>(handle);
                        if socket.can_recv() {
                            return socket.recv_slice(buf).map_err(|_| "recv failed");
                        }
                        if socket.state() == TcpState::Closed
                            || socket.state() == TcpState::CloseWait
                        {
                            return Ok(0);
                        }
                    }
                }
            }
            if crate::task::process::has_pending_signals() {
                return Err("interrupted");
            }
            yield_now();
        }
    }

    fn write(&self, inode: &mut Inode, _offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        let sock = {
            let map = INET_SOCKET_MAP.lock();
            map.get(&inode.id).cloned().ok_or("socket not found")?
        };

        loop {
            crate::net::stack::poll();
            {
                let s = sock.lock();
                if let Some(handle) = s.handle {
                    let mut guard = crate::net::stack::NET_STATE.lock();
                    if let Some(state) = guard.as_mut() {
                        let socket = state.sockets.get_mut::<TcpSocket>(handle);
                        if socket.can_send() {
                            let n = socket.send_slice(buf).map_err(|_| "send failed")?;
                            return Ok(n);
                        }
                    }
                }
            }
            if crate::task::process::has_pending_signals() {
                return Err("interrupted");
            }
            yield_now();
        }
    }

    fn readdir(&self, _inode: &Inode, _offset: u64) -> Result<Option<RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub fn create_inet_socket_file(sock: Arc<Mutex<InetSocket>>) -> File {
    let id = NEXT_INET_INODE.fetch_add(1, Relaxed);
    INET_SOCKET_MAP.lock().insert(id, sock);
    let inode = Inode {
        id,
        size: 0,
        file_type: FileType::Device,
        ops: &INET_INODE_OPS,
        file_ops: &INET_FILE_OPS,
    };
    File::new(
        inode,
        OpenFlags {
            read: true,
            write: true,
            create: false,
            truncate: false,
            append: false,
            directory: false,
            nonblock: false,
            exclusive: false,
        },
    )
}
