use core::ptr::{read_volatile, write_volatile};

use crate::arch::interrupts::consts::TICK_MS;
use crate::arch::interrupts::vectors::ticks;
use crate::kerror;

pub const CAP: usize = 0x00;
pub const GHC: usize = 0x04;
pub const PI: usize = 0x0c;
pub const VS: usize = 0x10;

pub const PORT_BASE: usize = 0x100;
pub const PORT_STRIDE: usize = 0x80;

pub const PX_CLB: usize = 0x00;
pub const PX_CLBU: usize = 0x04;
pub const PX_FB: usize = 0x08;
pub const PX_FBU: usize = 0x0c;
pub const PX_IS: usize = 0x10;
pub const PX_IE: usize = 0x14;
pub const PX_CMD: usize = 0x18;
pub const PX_TFD: usize = 0x20;
pub const PX_SSTS: usize = 0x28;
pub const PX_SERR: usize = 0x30;
pub const PX_CI: usize = 0x38;

pub const GHC_HR: u32 = 1 << 0;
pub const GHC_AE: u32 = 1 << 1;

// PxCMD: 0 ST, 1 SRE, 4 FRE are writable. 2/3 (spec) and 14/15 (emulator) are
// read-only engine-running bits, so either pair counts.
pub const CMD_ST: u32 = 1 << 0;
pub const CMD_SRE: u32 = 1 << 1;
pub const CMD_FRE: u32 = 1 << 4;
pub const CMD_RUNNING_SPEC: u32 = (1 << 2) | (1 << 3);
pub const CMD_RUNNING_EMULATOR: u32 = (1 << 14) | (1 << 15);
pub const CMD_RUNNING_ANY: u32 = CMD_RUNNING_SPEC | CMD_RUNNING_EMULATOR;

pub const TFD_ERR: u32 = 1 << 0;
pub const SSTS_DET_MASK: u32 = 0x0f;
pub const SSTS_DET_PHY_UP: u32 = 0x3;

// PxIS: each of these fires only while its PxIE bit is set, and clears on a
// write of 1. Bits 22 and 23 are reserved, so the bits a driver actually
// needs to watch are the two low errors, the completion bit at 8, and the
// host error block at 24-28.
pub const IS_ERR: u32 = 1 << 0;
pub const IS_HOST_BUS_DATA_ERROR: u32 = 1 << 1;
pub const IS_COMMAND_COMPLETE: u32 = 1 << 8;
pub const IS_INTERFACE_ERROR: u32 = 1 << 24;
pub const IS_HOST_BUS_FATAL_ERROR: u32 = 1 << 25;
pub const IS_HBA_FATAL_ERROR: u32 = 1 << 26;
pub const IS_SYSTEM_CONTROL_ERROR: u32 = 1 << 27;
pub const IS_DIAGNOSTIC: u32 = 1 << 28;

// The emulator latches its two fatal host errors one word above where the
// spec puts them, so a port can report either.
pub const IS_EMULATOR_FATAL: u32 = (1 << 30) | (1 << 31);

/// Unmasked in PxIE so any completion or error ends the wait. `arm` clears
/// PxIS first, so no stale bit can wake it.
pub const COMMAND_BITS: u32 = IS_ERR
    | IS_HOST_BUS_DATA_ERROR
    | IS_COMMAND_COMPLETE
    | IS_INTERFACE_ERROR
    | IS_HOST_BUS_FATAL_ERROR
    | IS_HBA_FATAL_ERROR
    | IS_SYSTEM_CONTROL_ERROR
    | IS_DIAGNOSTIC
    | IS_EMULATOR_FATAL;

/// The bits that mean it went wrong rather than finished.
pub const ERROR_BITS: u32 = COMMAND_BITS & !IS_COMMAND_COMPLETE;

pub const SECTOR: u64 = 512;

pub const ATA_IDENTIFY: u8 = 0xEC;
pub const ATA_READ_DMA_EXT: u8 = 0x25;
pub const ATA_WRITE_DMA_EXT: u8 = 0x35;
pub const FIS_H2D_REGISTER: u8 = 0x27;

pub fn reg(base: usize, offset: usize) -> u32 {
    unsafe { read_volatile((base + offset) as *const u32) }
}

pub fn set_reg(base: usize, offset: usize, value: u32) {
    unsafe { write_volatile((base + offset) as *mut u32, value) }
}

pub fn set_bits(base: usize, offset: usize, bits: u32) {
    set_reg(base, offset, reg(base, offset) | bits);
}

pub fn write_u32(buffer: *mut u8, offset: usize, value: u32) {
    unsafe { write_volatile(buffer.add(offset) as *mut u32, value) }
}

/// Waits up to `timeout_ms` for `condition`. The deadline counts timer ticks
/// rather than iterations, so it costs the same on a fast machine as a slow
/// one. Ticks need interrupts on, so they are enabled and then restored.
pub fn wait_for(timeout_ms: u64, mut condition: impl FnMut() -> bool) -> bool {
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

pub fn take_signature_error(base: usize) -> Option<&'static str> {
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
