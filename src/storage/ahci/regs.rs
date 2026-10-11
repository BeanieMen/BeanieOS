use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};

use crate::arch::interrupts::consts::TICK_MS;
use crate::arch::interrupts::vectors::ticks;
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
pub(crate) const PX_SSTS: usize = 0x28;
pub(crate) const PX_SERR: usize = 0x30;
pub(crate) const PX_CI: usize = 0x38;

pub(crate) const GHC_HR: u32 = 1 << 0;
pub(crate) const GHC_AE: u32 = 1 << 1;

// PxCMD: 2/3 (spec) and 14/15 (emulator) are read-only engine-running bits.
pub(crate) const CMD_ST: u32 = 1 << 0;
pub(crate) const CMD_SRE: u32 = 1 << 1;
pub(crate) const CMD_FRE: u32 = 1 << 4;
pub(crate) const CMD_RUNNING_SPEC: u32 = (1 << 2) | (1 << 3);
pub(crate) const CMD_RUNNING_EMULATOR: u32 = (1 << 14) | (1 << 15);
pub(crate) const CMD_RUNNING_ANY: u32 = CMD_RUNNING_SPEC | CMD_RUNNING_EMULATOR;

pub(crate) const TFD_ERR: u32 = 1 << 0;
pub(crate) const SSTS_DET_MASK: u32 = 0x0f;
pub(crate) const SSTS_DET_PHY_UP: u32 = 0x3;

// PxIS: fires only while its PxIE bit is set, clears on a write of 1. Bits 22-23
// are reserved, so the driver watches the two low errors, bit 8, and 24-28.
const IS_ERR: u32 = 1 << 0;
const IS_HOST_BUS_DATA_ERROR: u32 = 1 << 1;
const IS_COMMAND_COMPLETE: u32 = 1 << 8;
const IS_INTERFACE_ERROR: u32 = 1 << 24;
const IS_HOST_BUS_FATAL_ERROR: u32 = 1 << 25;
const IS_HBA_FATAL_ERROR: u32 = 1 << 26;
const IS_SYSTEM_CONTROL_ERROR: u32 = 1 << 27;
const IS_DIAGNOSTIC: u32 = 1 << 28;

// The emulator latches its two fatal host errors one word above the spec's.
const IS_EMULATOR_FATAL: u32 = (1 << 30) | (1 << 31);

// Unmasked in PxIE so any completion or error ends the wait. `arm` clears PxIS
// first, so no stale bit can wake it.
pub(crate) const COMMAND_BITS: u32 = IS_ERR
    | IS_HOST_BUS_DATA_ERROR
    | IS_COMMAND_COMPLETE
    | IS_INTERFACE_ERROR
    | IS_HOST_BUS_FATAL_ERROR
    | IS_HBA_FATAL_ERROR
    | IS_SYSTEM_CONTROL_ERROR
    | IS_DIAGNOSTIC
    | IS_EMULATOR_FATAL;

// The bits that mean it went wrong rather than finished.
pub(crate) const ERROR_BITS: u32 = COMMAND_BITS & !IS_COMMAND_COMPLETE;

pub(crate) const SECTOR: u64 = 512;

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

// Its neighbours in the command setup are `write_u32`, which is volatile, so a
// plain `copy_nonoverlapping` here is the one store LLVM is not obliged to keep
// where it was written -- symptom is a device reading a half-old FIS, no error.
pub(crate) fn copy_volatile(dest: *mut u8, src: &[u8]) {
    for (offset, byte) in src.iter().enumerate() {
        unsafe { dest.add(offset).write_volatile(*byte) };
    }
}

// The command list, table and transfer buffer are ordinary write-back memory the
// adapter reads over PCI, so `fence(SeqCst)` both stops LLVM moving the doorbell
// write above the setup and drains the store buffer. An `sfence` would cover the
// stores but not the later loads in `dma_consume`. Not redundant for the doorbell
// either: a write-combining buffer still takes stores out of order.
pub(crate) fn dma_publish() {
    fence(Ordering::SeqCst);
}

// Order the transfer buffer against the CPU. Same reasoning, mirrored.
pub(crate) fn dma_consume() {
    fence(Ordering::SeqCst);
}

// The deadline counts timer ticks, not iterations, so the wait costs the same on
// a fast machine as a slow one. Ticks need interrupts on: enabled, then restored.
pub(crate) fn wait_for(timeout_ms: u64, mut condition: impl FnMut() -> bool) -> bool {
    let was_enabled = x86_64::instructions::interrupts::are_enabled();

    if !was_enabled {
        x86_64::instructions::interrupts::enable();
    }

    let start = ticks();
    let limit = timeout_ms.div_ceil(TICK_MS);

    let satisfied = loop {
        if condition() {
            break true;
        }

        if ticks().wrapping_sub(start) >= limit {
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
