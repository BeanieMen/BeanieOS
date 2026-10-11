use core::cell::UnsafeCell;

use crate::memory::allocator::RESERVED;
use crate::memory::pool::Area;

pub(crate) struct Dma;

impl Dma {
    pub(crate) const fn new() -> Self {
        Dma
    }

    pub(crate) fn reserve(&self, region: Area) -> bool {
        RESERVED.push(region)
    }
}

impl Default for Dma {
    fn default() -> Self {
        Self::new()
    }
}

/// Command list: 32 entries of 4 bytes, one per port. AHCI wants the list
/// itself to be 1024 bytes even though 32 entries is only 128, so the spare
/// space is kept rather than relying on whatever follows in .bss.
const CLB_ENTRIES: usize = 32;
const CLB_ENTRY_SIZE: usize = 4;
const CLB_USED: usize = CLB_ENTRIES * CLB_ENTRY_SIZE;
const CLB_SIZE: usize = 1024;

/// Where the region descriptor table starts in a command table.
pub(crate) const CMD_TBL_HDR: usize = 0x80;

/// One region descriptor entry.
pub(crate) const PRD_SIZE: u32 = 16;
pub(crate) const PRD_COUNT: u32 = 1;

/// The adapter writes completions here; the size only has to cover one.
const FB_SIZE: usize = 256;

/// Must be at least one sector, since the driver reads whole sectors into it.
pub(crate) const BUF_SIZE: usize = crate::paging::PAGE_SIZE as usize;

/// Alignment each structure needs, so the adapter accepts the addresses.
#[repr(C, align(1024))]
pub(crate) struct CommandList(pub [u8; CLB_SIZE]);

#[repr(C, align(128))]
pub(crate) struct CommandTable(pub [u8; CMD_TBL_HDR + 16]);

#[repr(C, align(256))]
pub(crate) struct ReceiveArea(pub [u8; FB_SIZE]);

#[repr(C, align(4096))]
pub(crate) struct TransferBuffer(pub [u8; BUF_SIZE]);

#[repr(C, align(4096))]
pub(crate) struct AhciDma {
    command_list: UnsafeCell<CommandList>,
    command_table: UnsafeCell<CommandTable>,
    receive_area: UnsafeCell<ReceiveArea>,
    transfer_buffer: UnsafeCell<TransferBuffer>,
}

// SAFETY: reached through raw pointers, single threaded, one transfer at a time.
unsafe impl Sync for AhciDma {}

impl AhciDma {
    pub(crate) fn command_list(&self) -> *mut CommandList {
        self.command_list.get()
    }

    pub(crate) fn command_table(&self) -> *mut CommandTable {
        self.command_table.get()
    }

    pub(crate) fn receive_area(&self) -> *mut ReceiveArea {
        self.receive_area.get()
    }

    pub(crate) fn transfer_buffer(&self) -> *mut TransferBuffer {
        self.transfer_buffer.get()
    }

    pub(crate) fn clear(&self) {
        for (buffer, size) in [
            (self.command_list.get() as *mut u8, CLB_USED),
            (self.command_table.get() as *mut u8, CMD_TBL_HDR + 16),
            (self.receive_area.get() as *mut u8, FB_SIZE),
        ] {
            unsafe { core::ptr::write_bytes(buffer, 0, size) };
        }
    }
}

pub(crate) static AHCI_DMA: AhciDma = AhciDma {
    command_list: UnsafeCell::new(CommandList([0; CLB_SIZE])),
    command_table: UnsafeCell::new(CommandTable([0; CMD_TBL_HDR + 16])),
    receive_area: UnsafeCell::new(ReceiveArea([0; FB_SIZE])),
    transfer_buffer: UnsafeCell::new(TransferBuffer([0; BUF_SIZE])),
};
