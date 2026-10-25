use alloc::{
    collections::VecDeque,
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::mutex::Mutex;

pub enum SocketState {
    Unbound,
    Listening {
        backlog: usize,
        incoming: VecDeque<Arc<Mutex<UnixSocket>>>,
    },
    Connected {
        peer: Weak<Mutex<UnixSocket>>,
        rx_buffer: VecDeque<u8>,
        peer_closed: bool,
    },
    Closed,
}

pub struct UnixSocket {
    pub state: SocketState,
    pub bound_path: Option<Vec<u8>>,
}

impl UnixSocket {
    pub fn new() -> Self {
        Self {
            state: SocketState::Unbound,
            bound_path: None,
        }
    }
}

// key references the parent thats why explicit send and sync is required
unsafe impl Send for UnixSocket {}
unsafe impl Sync for UnixSocket {}

