use core::cell::UnsafeCell;

/// Sizes and alignments the spec requires of each bus structure.
pub const CLB_SIZE: usize = 32 * 32; // 32 slots of 32 bytes
pub const CMD_TBL_HDR: usize = 0x80; // region table starts here
pub const FB_SIZE: usize = 256;
pub const BUF_SIZE: usize = 4096; // page aligned: no region straddles a page

/// How many region descriptors one command uses.
pub const PRD_COUNT: u32 = 1;

/// Byte count of one region descriptor, which is what the spec's length
/// field in the command table counts.
pub const PRD_SIZE: u32 = 16;

#[repr(C, align(1024))]
pub struct CommandList(pub [u8; CLB_SIZE]);

#[repr(C, align(128))]
pub struct CommandTable(pub [u8; CMD_TBL_HDR + 16]);

#[repr(C, align(256))]
pub struct ReceiveArea(pub [u8; FB_SIZE]);

#[repr(C, align(4096))]
pub struct TransferBuffer(pub [u8; BUF_SIZE]);

/// One block, shared by every disk: only one transfer is in flight at a time.
#[repr(C, align(4096))]
pub struct Dma {
    command_list: UnsafeCell<CommandList>,
    command_table: UnsafeCell<CommandTable>,
    receive_area: UnsafeCell<ReceiveArea>,
    transfer_buffer: UnsafeCell<TransferBuffer>,
}

// SAFETY: reached through raw pointers, single threaded, one transfer at a time.
unsafe impl Sync for Dma {}

impl Dma {
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

pub static DMA: Dma = Dma {
    command_list: UnsafeCell::new(CommandList([0; CLB_SIZE])),
    command_table: UnsafeCell::new(CommandTable([0; CMD_TBL_HDR + 16])),
    receive_area: UnsafeCell::new(ReceiveArea([0; FB_SIZE])),
    transfer_buffer: UnsafeCell::new(TransferBuffer([0; BUF_SIZE])),
};
