use alloc::collections::VecDeque;
use alloc::sync::Arc;
use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::arch::interrupts::consts::{PS2_DATA_PORT, PS2_STATUS_PORT};
use crate::arch::lock::InterruptMutex;
use crate::storage::vfs::{
    file::{File, OpenFlags},
    inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry},
};

const STATUS_OUTPUT_FULL: u8 = 1 << 0;
const STATUS_INPUT_FULL: u8 = 1 << 1;
const POLL_LIMIT: u32 = 100_000;

fn status() -> u8 {
    unsafe { Port::<u8>::new(PS2_STATUS_PORT).read() }
}

fn wait_input_empty() -> bool {
    for _ in 0..POLL_LIMIT {
        if status() & STATUS_INPUT_FULL == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn wait_output_full() -> bool {
    for _ in 0..POLL_LIMIT {
        if status() & STATUS_OUTPUT_FULL != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn mouse_write(byte: u8) {
    wait_input_empty();
    unsafe {
        Port::<u8>::new(PS2_STATUS_PORT).write(0xD4);
    }
    wait_input_empty();
    unsafe {
        Port::<u8>::new(PS2_DATA_PORT).write(byte);
    }
}

fn mouse_read() -> Option<u8> {
    if wait_output_full() {
        Some(unsafe { Port::<u8>::new(PS2_DATA_PORT).read() })
    } else {
        None
    }
}

pub(crate) fn init() {
    // Drain any leftover bytes from PS/2 output buffer
    let mut drained = 0;
    while drained < 16 && (status() & STATUS_OUTPUT_FULL != 0) {
        unsafe {
            Port::<u8>::new(PS2_DATA_PORT).read();
        }
        drained += 1;
    }

    // Enable auxiliary device (mouse port) on PS/2 controller
    wait_input_empty();
    unsafe {
        Port::<u8>::new(PS2_STATUS_PORT).write(0xA8);
    }

    // Read Controller Configuration Byte
    wait_input_empty();
    unsafe {
        Port::<u8>::new(PS2_STATUS_PORT).write(0x20);
    }
    let mut config = if let Some(b) = mouse_read() {
        b
    } else {
        crate::kwarn!("mouse: failed to read PS/2 config byte");
        return;
    };

    // Enable IRQ 1 (bit 0), IRQ 12 (bit 1)
    // Enable clock for port 1 (clear bit 4) and port 2 (clear bit 5)
    // Enable translation for port 1 (bit 6)
    config |= 0x01;
    config |= 0x02;
    config &= !0x10;
    config &= !0x20;
    config |= 0x40;

    wait_input_empty();
    unsafe {
        Port::<u8>::new(PS2_STATUS_PORT).write(0x60);
    }
    wait_input_empty();
    unsafe {
        Port::<u8>::new(PS2_DATA_PORT).write(config);
    }

    // Set Defaults (0xF6)
    mouse_write(0xF6);
    let _ = mouse_read(); // ACK (0xFA)

    // Enable Data Reporting (0xF4)
    mouse_write(0xF4);
    let _ = mouse_read(); // ACK (0xFA)

    // Drain any leftover bytes after enabling reporting
    let mut drained = 0;
    while drained < 16 && (status() & STATUS_OUTPUT_FULL != 0) {
        unsafe {
            Port::<u8>::new(PS2_DATA_PORT).read();
        }
        drained += 1;
    }

    crate::kinfo!("mouse: PS/2 mouse initialized and streaming enabled");
}

#[derive(Clone, Copy, Default)]
struct PacketState {
    buf: [u8; 3],
    idx: usize,
}

static PACKET_STATE: InterruptMutex<PacketState> = InterruptMutex::new(PacketState {
    buf: [0; 3],
    idx: 0,
});

pub fn handle_mouse_byte(byte: u8) {
    let mut state = PACKET_STATE.lock();

    // Byte 0 must have bit 3 set to 1 for synchronization
    if state.idx == 0 && (byte & 0x08) == 0 {
        return;
    }

    let idx = state.idx;
    state.buf[idx] = byte;
    state.idx += 1;

    if state.idx == 3 {
        state.idx = 0;
        let flags = state.buf[0];
        let raw_x = state.buf[1];
        let raw_y = state.buf[2];

        // Check overflow (bits 6, 7)
        if (flags & 0xC0) != 0 {
            return;
        }

        let mut dx = raw_x as i16;
        if (flags & 0x10) != 0 {
            dx |= !0xFF;
        }

        let mut dy = raw_y as i16;
        if (flags & 0x20) != 0 {
            dy |= !0xFF;
        }

        let buttons = flags & 0x07; // bit 0 = Left, bit 1 = Right, bit 2 = Middle
        let dx_clamped = dx.clamp(-128, 127) as i8;
        let dy_clamped = dy.clamp(-128, 127) as i8;

        push_mouse_event(buttons, dx_clamped, dy_clamped);
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MouseEvent {
    pub buttons: u8,
    pub dx: i8,
    pub dy: i8,
    pub _reserved: u8,
}

pub static MOUSE_QUEUE: InterruptMutex<VecDeque<MouseEvent>> =
    InterruptMutex::new(VecDeque::new());

pub fn push_mouse_event(buttons: u8, dx: i8, dy: i8) {
    let mut q = MOUSE_QUEUE.lock();
    if q.len() < 256 {
        q.push_back(MouseEvent {
            buttons,
            dx,
            dy,
            _reserved: 0,
        });
    }
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
        if buf.len() < 4 {
            return Ok(0);
        }
        let mut q = MOUSE_QUEUE.lock();
        if let Some(ev) = q.pop_front() {
            buf[0] = ev.buttons;
            buf[1] = ev.dx as u8;
            buf[2] = ev.dy as u8;
            buf[3] = 0;
            Ok(4)
        } else {
            Ok(0)
        }
    }
    fn write(&self, _inode: &mut Inode, _offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
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
        id: 4,
        file_type: FileType::Device,
        size: 0,
        cluster: 0,
        inode_ops: &MOUSE_INODE_OPS,
        file_ops: &MOUSE_FILE_OPS,
        dentry: None,
    };
    File::new(Arc::new(Mutex::new(inode)), flags)
}
