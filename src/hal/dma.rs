use core::cell::UnsafeCell;

use spin::Mutex;

#[derive(Clone, Copy)]
pub struct Region {
    pub start: u64,
    pub end: u64,
}

impl Region {
    pub const fn new(start: u64, end: u64) -> Self {
        Region { start, end }
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }
}

const MAX_RESERVED: usize = 32;

struct ReservedList {
    entries: [Region; MAX_RESERVED],
    count: usize,
}

impl ReservedList {
    const fn new() -> Self {
        ReservedList {
            entries: [Region::new(0, 0); MAX_RESERVED],
            count: 0,
        }
    }

    fn push(&mut self, region: Region) {
        if self.count >= MAX_RESERVED {
            return;
        }

        self.entries[self.count] = region;
        self.count += 1;
    }
}

pub struct Dma {
    reserved: Mutex<ReservedList>,
}

impl Dma {
    pub const fn new() -> Self {
        Dma {
            reserved: Mutex::new(ReservedList::new()),
        }
    }

    pub fn reserve(&self, region: Region) {
        self.reserved.lock().push(region);
    }

    /// Feeds every claimed range to `visit` without allocating, so this runs
    /// before the heap exists.
    pub fn copy_reserved(&self, mut visit: impl FnMut(Region)) {
        let guard = self.reserved.lock();

        for region in &guard.entries[..guard.count] {
            visit(*region);
        }
    }
}

impl Default for Dma {
    fn default() -> Self {
        Self::new()
    }
}

pub const CLB_SIZE: usize = 32 * 32;
pub const CMD_TBL_HDR: usize = 0x80;
pub const FB_SIZE: usize = 256;
pub const BUF_SIZE: usize = 4096;
pub const PRD_COUNT: u32 = 1;
pub const PRD_SIZE: u32 = 16;

#[repr(C, align(1024))]
pub struct CommandList(pub [u8; CLB_SIZE]);

#[repr(C, align(128))]
pub struct CommandTable(pub [u8; CMD_TBL_HDR + 16]);

#[repr(C, align(256))]
pub struct ReceiveArea(pub [u8; FB_SIZE]);

#[repr(C, align(4096))]
pub struct TransferBuffer(pub [u8; BUF_SIZE]);

#[repr(C, align(4096))]
pub struct AhciDma {
    command_list: UnsafeCell<CommandList>,
    command_table: UnsafeCell<CommandTable>,
    receive_area: UnsafeCell<ReceiveArea>,
    transfer_buffer: UnsafeCell<TransferBuffer>,
}

// SAFETY: reached through raw pointers, single threaded, one transfer at a time.
unsafe impl Sync for AhciDma {}

impl AhciDma {
    pub fn command_list(&self) -> *mut CommandList {
        self.command_list.get()
    }

    pub fn command_table(&self) -> *mut CommandTable {
        self.command_table.get()
    }

    pub fn receive_area(&self) -> *mut ReceiveArea {
        self.receive_area.get()
    }

    pub fn transfer_buffer(&self) -> *mut TransferBuffer {
        self.transfer_buffer.get()
    }

    pub fn clear(&self) {
        for (buffer, size) in [
            (self.command_list.get() as *mut u8, CLB_SIZE),
            (self.command_table.get() as *mut u8, CMD_TBL_HDR + 16),
            (self.receive_area.get() as *mut u8, FB_SIZE),
        ] {
            unsafe { core::ptr::write_bytes(buffer, 0, size) };
        }
    }
}

pub static AHCI_DMA: AhciDma = AhciDma {
    command_list: UnsafeCell::new(CommandList([0; CLB_SIZE])),
    command_table: UnsafeCell::new(CommandTable([0; CMD_TBL_HDR + 16])),
    receive_area: UnsafeCell::new(ReceiveArea([0; FB_SIZE])),
    transfer_buffer: UnsafeCell::new(TransferBuffer([0; BUF_SIZE])),
};