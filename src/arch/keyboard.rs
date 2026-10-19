use x86_64::instructions::port::Port;

use crate::arch::interrupts::consts::{PS2_DATA_PORT, PS2_STATUS_PORT};

const STATUS_OUTPUT_FULL: u8 = 1 << 0;

const KEYBOARD_ACK: u8 = 0xFA;
const KEYBOARD_ENABLE_SCANNING: u8 = 0xF4;

const POLL_LIMIT: u32 = 100_000;

fn status() -> u8 {
    unsafe { Port::<u8>::new(PS2_STATUS_PORT).read() }
}

fn read_data() -> u8 {
    unsafe { Port::<u8>::new(PS2_DATA_PORT).read() }
}

fn send_keyboard_command(byte: u8) {
    unsafe { Port::<u8>::new(PS2_DATA_PORT).write(byte) }
}

fn wait_for(predicate: impl Fn(u8) -> bool) -> bool {
    for _ in 0..POLL_LIMIT {
        if predicate(status()) {
            return true;
        }

        core::hint::spin_loop();
    }

    false
}

fn wait_output_empty() -> bool {
    wait_for(|status| status & STATUS_OUTPUT_FULL == 0)
}

fn wait_output_full() -> bool {
    wait_for(|status| status & STATUS_OUTPUT_FULL != 0)
}

pub(crate) fn init() {
    let mut drained = 0;

    while drained < POLL_LIMIT && wait_output_full() {
        read_data();
        drained += 1;
    }

    if !wait_output_empty() {
        crate::kwarn!("keyboard: output buffer never drained");
        return;
    }

    send_keyboard_command(KEYBOARD_ENABLE_SCANNING);

    if !wait_output_full() {
        crate::kwarn!("keyboard: controller did not answer the enable command");
        return;
    }

    let ack = read_data();

    if ack != KEYBOARD_ACK {
        crate::kwarn!("keyboard: enable answered {ack:#04x} instead of an ACK");
        return;
    }

    crate::kinfo!("keyboard: scanning enabled");
}

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use spin::Mutex;

use crate::arch::lock::InterruptMutex;
use crate::storage::vfs::{
    file::{File, OpenFlags},
    inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry},
};

pub static KBD_QUEUE: InterruptMutex<VecDeque<(u8, u8)>> = InterruptMutex::new(VecDeque::new());

pub fn push_event(pressed: u8, key: u8) {
    let mut q = KBD_QUEUE.lock();
    if q.len() < 256 {
        q.push_back((pressed, key));
    }
}

pub struct KbdInodeOps;
impl InodeOperations for KbdInodeOps {
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

pub struct KbdFileOps;
impl FileOperations for KbdFileOps {
    fn read(&self, _inode: &Inode, _offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.len() < 2 {
            return Ok(0);
        }
        let mut q = KBD_QUEUE.lock();
        if let Some((pressed, key)) = q.pop_front() {
            buf[0] = pressed;
            buf[1] = key;
            Ok(2)
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

pub static KBD_INODE_OPS: KbdInodeOps = KbdInodeOps;
pub static KBD_FILE_OPS: KbdFileOps = KbdFileOps;

pub fn create_kbd_file(flags: OpenFlags) -> File {
    let inode = Inode {
        id: 3,
        file_type: FileType::Device,
        size: 0,
        cluster: 0,
        inode_ops: &KBD_INODE_OPS,
        file_ops: &KBD_FILE_OPS,
        dentry: None,
    };
    File::new(Arc::new(Mutex::new(inode)), flags)
}
