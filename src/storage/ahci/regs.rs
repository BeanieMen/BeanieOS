use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

use crate::arch::interrupts::consts::TICK_MS;
use crate::arch::interrupts::timer_ticks;
use crate::kerror;

pub(crate) const CAP: usize = 0x00;
pub(crate) const GHC: usize = 0x04;
pub(crate) const PI: usize = 0x0c;
pub(crate) const VS: usize = 0x10;

pub(crate) const PORT_BASE: usize = 0x100;
pub(crate) const PORT_STRIDE: usize = 0x80;

pub(crate) const PX_CLB: usize = 0x00;
pub(crate) const PX_CLBU: usize = 0x04;
pub(crate) const PX_FB: usize = 0x08;
pub(crate) const PX_FBU: usize = 0x0c;
pub(crate) const PX_IS: usize = 0x10;
pub(crate) const PX_IE: usize = 0x14;
pub(crate) const PX_CMD: usize = 0x18;
pub(crate) const PX_TFD: usize = 0x20;
pub(crate) const PX_SIG: usize = 0x24;
pub(crate) const PX_SSTS: usize = 0x28;
pub(crate) const PX_SERR: usize = 0x30;
pub(crate) const PX_CI: usize = 0x38;

pub(crate) const GHC_HR: u32 = 1 << 0;
pub(crate) const GHC_AE: u32 = 1 << 1;

pub(crate) const CMD_ST: u32 = 1 << 0;
pub(crate) const CMD_SRE: u32 = 1 << 1;
pub(crate) const CMD_FRE: u32 = 1 << 4;
pub(crate) const CMD_RUNNING_SPEC: u32 = (1 << 2) | (1 << 3);
pub(crate) const CMD_RUNNING_EMULATOR: u32 = (1 << 14) | (1 << 15);
pub(crate) const CMD_RUNNING_ANY: u32 = CMD_RUNNING_SPEC | CMD_RUNNING_EMULATOR;

pub(crate) const TFD_ERR: u32 = 1 << 0;
pub(crate) const SSTS_DET_MASK: u32 = 0x0f;
pub(crate) const SSTS_DET_PHY_UP: u32 = 0x3;

const IS_D2H_REG_FIS: u32 = 1 << 0;
const IS_PIO_SETUP_FIS: u32 = 1 << 1;
const IS_DMA_SETUP_FIS: u32 = 1 << 2;
const IS_SET_DEVICE_BITS_FIS: u32 = 1 << 3;
const IS_UNKNOWN_FIS: u32 = 1 << 4;
const IS_SG_DONE: u32 = 1 << 5;
const IS_PORT_CONNECT_CHANGE: u32 = 1 << 6;
const IS_PHYRDY_CHANGE: u32 = 1 << 22;
const IS_BAD_PMP: u32 = 1 << 23;
const IS_OVERFLOW: u32 = 1 << 24;
const IS_INTERFACE_NONFATAL: u32 = 1 << 26;
const IS_INTERFACE_FATAL: u32 = 1 << 27;
const IS_HOST_BUS_DATA_ERROR: u32 = 1 << 28;
const IS_HOST_BUS_FATAL_ERROR: u32 = 1 << 29;
const IS_TASK_FILE_ERROR: u32 = 1 << 30;

pub(crate) const COMMAND_BITS: u32 = IS_TASK_FILE_ERROR
    | IS_HOST_BUS_FATAL_ERROR
    | IS_HOST_BUS_DATA_ERROR
    | IS_INTERFACE_FATAL
    | IS_INTERFACE_NONFATAL
    | IS_OVERFLOW
    | IS_BAD_PMP
    | IS_PHYRDY_CHANGE
    | IS_PORT_CONNECT_CHANGE
    | IS_SG_DONE
    | IS_UNKNOWN_FIS
    | IS_SET_DEVICE_BITS_FIS
    | IS_DMA_SETUP_FIS
    | IS_PIO_SETUP_FIS
    | IS_D2H_REG_FIS;

pub(crate) const ERROR_BITS: u32 = IS_TASK_FILE_ERROR
    | IS_HOST_BUS_FATAL_ERROR
    | IS_HOST_BUS_DATA_ERROR
    | IS_INTERFACE_FATAL
    | IS_INTERFACE_NONFATAL
    | IS_OVERFLOW
    | IS_BAD_PMP
    | IS_PHYRDY_CHANGE
    | IS_PORT_CONNECT_CHANGE
    | IS_UNKNOWN_FIS;

pub(crate) const SECTOR: u64 = 512;

pub(crate) const SATA_SIGNATURE_DISK: u32 = 0x0000_0101;
pub(crate) const SATA_SIGNATURE_CDROM: u32 = 0xeb14_0101;

pub(crate) const ATA_IDENTIFY: u8 = 0xEC;
pub(crate) const ATA_READ_DMA_EXT: u8 = 0x25;
pub(crate) const ATA_WRITE_DMA_EXT: u8 = 0x35;
pub(crate) const FIS_H2D_REGISTER: u8 = 0x27;

pub(crate) fn reg(base: usize, offset: usize) -> u32 {
    unsafe { read_volatile((base + offset) as *const u32) }
}

pub(crate) fn set_reg(base: usize, offset: usize, value: u32) {
    unsafe { write_volatile((base + offset) as *mut u32, value) }
}

pub(crate) fn set_bits(base: usize, offset: usize, bits: u32) {
    set_reg(base, offset, reg(base, offset) | bits);
}

pub(crate) fn write_u32(buffer: *mut u8, offset: usize, value: u32) {
    unsafe { write_volatile(buffer.add(offset) as *mut u32, value) }
}

// The one store LLVM is not obliged to keep where it was written: symptom is a
// device reading a half-old FIS, with no error reported.
pub(crate) fn copy_volatile(dest: *mut u8, src: &[u8]) {
    for (offset, byte) in src.iter().enumerate() {
        unsafe { dest.add(offset).write_volatile(*byte) };
    }
}

// The command list, table and transfer buffer are write-back memory the adapter
// reads over PCI, so this stops LLVM moving the doorbell above the setup and
// drains the store buffer. Not redundant for the doorbell either: a
// write-combining buffer still takes stores out of order.
pub(crate) fn dma_publish() {
    fence(Ordering::SeqCst);
}

pub(crate) fn dma_consume() {
    fence(Ordering::SeqCst);
}

pub(crate) fn wait_for(timeout_ms: u64, mut condition: impl FnMut() -> bool) -> bool {
    let was_enabled = x86_64::instructions::interrupts::are_enabled();

    if !was_enabled {
        x86_64::instructions::interrupts::enable();
    }

    let start = timer_ticks();
    let limit = timeout_ms.div_ceil(TICK_MS);

    let satisfied = loop {
        if condition() {
            break true;
        }

        if timer_ticks().wrapping_sub(start) >= limit {
            break false;
        }

        core::hint::spin_loop();
    };

    if !was_enabled {
        x86_64::instructions::interrupts::disable();
    }

    satisfied
}

pub(crate) fn take_signature_error(base: usize) -> Option<&'static str> {
    let raw = reg(base, PX_SERR);

    if raw == 0 {
        return None;
    }

    set_reg(base, PX_SERR, raw);

    let named = if raw & (1 << 0) != 0 {
        "host rejected the command FIS"
    } else if raw & (1 << 1) != 0 {
        "host rejected the command table"
    } else if raw & (1 << 2) != 0 {
        "host rejected the data region"
    } else if raw & (1 << 3) != 0 {
        "host rejected the FIS receive area"
    } else if raw & (1 << 4) != 0 {
        "host bus data error"
    } else {
        "host reported an unrecognised error"
    };

    kerror!("PxSERR={raw:#x}: {named}");

    Some(named)
}
