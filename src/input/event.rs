use alloc::{collections::VecDeque, sync::Arc};
use spin::Mutex;

use crate::{
    arch::lock::InterruptMutex,
    storage::vfs::{
        file::{File, OpenFlags},
        inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry},
    },
};

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const SYN_REPORT: u16 = 0;

pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct InputEvent {
    pub time: Timeval,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

pub static EVENT_QUEUE: InterruptMutex<VecDeque<InputEvent>> = InterruptMutex::new(VecDeque::new());

pub fn push_event(type_: u16, code: u16, value: i32) {
    let ticks = crate::arch::interrupts::vectors::ticks();
    let ev = InputEvent {
        time: Timeval {
            tv_sec: (ticks / 1000) as i64,
            tv_usec: ((ticks % 1000) * 1000) as i64,
        },
        type_,
        code,
        value,
    };

    let mut q = EVENT_QUEUE.lock();
    if q.len() < 512 {
        q.push_back(ev);
    }
}

pub fn push_key(code: u16, value: i32) {
    push_event(EV_KEY, code, value);
    push_event(EV_SYN, SYN_REPORT, 0)
}

pub fn push_rel(axis: u16, delta: i32) {
    if delta != 0 {
        push_event(EV_REL, axis, delta)
    }
}

pub fn push_button(button: u16, pressed: i32) {
    push_event(EV_KEY, button, pressed);
}

pub fn push_syn() {
    push_event(EV_SYN, SYN_REPORT, 0);
}

pub struct EventInodeOps;
impl InodeOperations for EventInodeOps {
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
pub struct EventFileOps;
impl FileOperations for EventFileOps {
    fn read(&self, _inode: &Inode, _offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        let event_size = core::mem::size_of::<InputEvent>();
        if buf.len() < event_size {
            return Ok(0);
        }
        let mut q = EVENT_QUEUE.lock();
        let mut bytes_written = 0;
        while bytes_written + event_size <= buf.len() {
            if let Some(ev) = q.pop_front() {
                let ev_bytes: &[u8] = unsafe {
                    core::slice::from_raw_parts(&ev as *const _ as *const u8, event_size)
                };
                buf[bytes_written..bytes_written + event_size].copy_from_slice(ev_bytes);
                bytes_written += event_size;
            } else {
                break;
            }
        }
        Ok(bytes_written)
    }
    fn write(&self, _inode: &mut Inode, _offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn readdir(&self, _inode: &Inode, _offset: u64) -> Result<Option<RawEntry>, &'static str> {
        Err("not a directory")
    }
}
pub static EVENT_INODE_OPS: EventInodeOps = EventInodeOps;
pub static EVENT_FILE_OPS: EventFileOps = EventFileOps;

pub fn create_event_file(flags: OpenFlags) -> File {
    let inode = Inode {
        id: 6,
        file_type: FileType::Device,
        size: 0,
        cluster: 0,
        inode_ops: &EVENT_INODE_OPS,
        file_ops: &EVENT_FILE_OPS,
        dentry: None,
    };
    File::new(Arc::new(Mutex::new(inode)), flags)
}
