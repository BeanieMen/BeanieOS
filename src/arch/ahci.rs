//! AHCI (Serial ATA) driver, written from scratch against AHCI 1.3.
//!
//! Every structure the controller fetches over the bus is a `static` below.
//! They live in the kernel image, which loads around 1 MiB and is inside the
//! 0..8 GiB window the boot page tables identity map, so virtual and physical
//! addresses are the same number. No address translation is needed, and no
//! contiguity requirement is placed on the heap.

use core::ptr::{read_volatile, write_volatile};

use crate::arch::pci::Device;
use crate::println;

const CAP: usize = 0x00;
const GHC: usize = 0x04;
const PI: usize = 0x0c;
const VS: usize = 0x10;

const PORT_BASE: usize = 0x100;
const PORT_STRIDE: usize = 0x80;

const PxCLB: usize = 0x00;
const PxCLBU: usize = 0x04;
const PxFB: usize = 0x08;
const PxFBU: usize = 0x0c;
const PxIS: usize = 0x10;
const PxIE: usize = 0x14;
const PxCMD: usize = 0x18;
const PxTFD: usize = 0x20;
const PxSSTS: usize = 0x28;
const PxSERR: usize = 0x30;
const PxCI: usize = 0x38;

const GHC_HR: u32 = 1 << 0;
const GHC_AE: u32 = 1 << 1;

// Port command register: 0 ST, 1 SRE, 4 FRE. 2, 3, 14 and 15 are read-only
// and only ever polled. The specification reports the engines running on 2
// and 3, the emulator on 14 and 15, so either pair counts.
const CMD_ST: u32 = 1 << 0;
const CMD_SRE: u32 = 1 << 1;
const CMD_FRE: u32 = 1 << 4;
const CMD_RUNNING_SPEC: u32 = (1 << 2) | (1 << 3);
const CMD_RUNNING_EMULATOR: u32 = (1 << 14) | (1 << 15);
const CMD_RUNNING_ANY: u32 = CMD_RUNNING_SPEC | CMD_RUNNING_EMULATOR;

const TFD_ERR: u32 = 1 << 0;
const SSTS_DET_MASK: u32 = 0x0f;
const SSTS_DET_PHY_UP: u32 = 0x3;

const ATA_IDENTIFY: u8 = 0xEC;
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;
const FIS_H2D_REGISTER: u8 = 0x27;

/// Sizes and alignments the specification requires of each bus structure.
const CLB_SIZE: usize = 32 * 32; // 32 slots of 32 bytes
const CMD_TBL_HDR: usize = 0x80; // region table starts here
const FB_SIZE: usize = 256;
const BUF_SIZE: usize = 4096; // page aligned: no region may straddle a page

#[repr(C, align(1024))]
struct CommandList([u8; CLB_SIZE]);
#[repr(C, align(128))]
struct CommandTable([u8; CMD_TBL_HDR + 16]);
#[repr(C, align(256))]
struct ReceiveArea([u8; FB_SIZE]);
#[repr(C, align(4096))]
struct TransferBuffer([u8; BUF_SIZE]);

static mut COMMAND_LIST: CommandList = CommandList([0; CLB_SIZE]);
static mut COMMAND_TABLE: CommandTable = CommandTable([0; CMD_TBL_HDR + 16]);
static mut RECEIVE_AREA: ReceiveArea = ReceiveArea([0; FB_SIZE]);
static mut TRANSFER_BUFFER: TransferBuffer = TransferBuffer([0; BUF_SIZE]);

pub struct AhciController {
    abar: usize,
    port: usize,
    sectors: u64,
    block_size: usize,
    lba48: bool,
}

fn reg(base: usize, offset: usize) -> u32 {
    unsafe { read_volatile((base + offset) as *const u32) }
}

fn set_reg(base: usize, offset: usize, value: u32) {
    unsafe { write_volatile((base + offset) as *mut u32, value) }
}

/// Read-modify-write, because the read-only running bits share the register.
fn set_bits(base: usize, offset: usize, bits: u32) {
    set_reg(base, offset, reg(base, offset) | bits);
}

fn write_u32(buffer: *mut u8, offset: usize, value: u32) {
    unsafe { write_volatile(buffer.add(offset) as *mut u32, value) }
}

/// Bounded poll. Counting iterations rather than milliseconds keeps this
/// independent of any timer, and a missing device cannot hang the kernel.
fn wait_for(rounds: u64, mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..rounds {
        if condition() {
            return true;
        }
        core::hint::spin_loop();
    }
    condition()
}

impl AhciController {
    pub fn new(device: &Device) -> Result<Self, &'static str> {
        let abar = device.ahci_base().ok_or("no BAR5")?;

        // The BAR is not decoded and the device cannot do bus mastering until
        // these bits are set. Reading the BAR to size it writes to the BAR, so
        // this has to come after any probing.
        device.enable();

        let capabilities = reg(abar, CAP);
        let version = reg(abar, VS);
        let implemented = reg(abar, PI);

        // Both capability fields are zero-based. The parentheses matter: `&`
        // binds looser than `+`, so without them this tests one bit instead.
        let slots = ((capabilities >> 8) & 0x1f) + 1;
        let wide = if capabilities & 1 != 0 { "yes" } else { "no" };
        let major = version >> 16;
        let minor = version & 0xffff;

        println!(
            "AHCI at {abar:#x} v{major}.{minor}, ports {implemented:#b}, \
             {} slots, 64 bit {wide}",
            slots,
        );

        set_reg(abar, GHC, GHC_HR);
        if !wait_for(10_000_000, || reg(abar, GHC) & GHC_HR == 0) {
            return Err("HBA reset did not complete");
        }

        // Interrupts are deliberately left disabled: this driver polls, and an
        // interrupt routed to an unhandled vector is worse than a slow command.
        set_reg(abar, GHC, GHC_AE);

        // The boot page tables do not clear .bss for us.
        for (buffer, size) in [
            (&raw const COMMAND_LIST as *mut u8, CLB_SIZE),
            (&raw const COMMAND_TABLE as *mut u8, CMD_TBL_HDR + 16),
            (&raw const RECEIVE_AREA as *mut u8, FB_SIZE),
        ] {
            unsafe { core::ptr::write_bytes(buffer, 0, size) };
        }

        for port in 0..32 {
            if implemented & (1 << port) == 0 {
                continue;
            }
            match Self::attach(abar, port) {
                Ok(controller) => return Ok(controller),
                Err(why) => println!("  port {port}: {why}"),
            }
        }

        Err("no port reported a device")
    }

    fn attach(abar: usize, port: usize) -> Result<Self, &'static str> {
        let base = abar + PORT_BASE + port * PORT_STRIDE;

        set_reg(base, PxCMD, 0);
        if !wait_for(10_000_000, || reg(base, PxCMD) & CMD_RUNNING_ANY == 0) {
            return Err("did not stop");
        }

        // Identity mapped, so these addresses are the physical addresses.
        let clb = &raw const COMMAND_LIST as u64;
        let fb = &raw const RECEIVE_AREA as u64;
        set_reg(base, PxCLB, clb as u32);
        set_reg(base, PxCLBU, (clb >> 32) as u32);
        set_reg(base, PxFB, fb as u32);
        set_reg(base, PxFBU, (fb >> 32) as u32);

        set_bits(base, PxCMD, CMD_SRE);

        // The staggered spin-up bit is read-only in the emulator, so the link
        // is the only usable evidence. 3 means present and physical layer up.
        if !wait_for(50_000_000, || reg(base, PxSSTS) & SSTS_DET_MASK == SSTS_DET_PHY_UP) {
            return Err(match reg(base, PxSSTS) & SSTS_DET_MASK {
                0 => "no device",
                _ => "link did not come up",
            });
        }

        set_reg(base, PxIE, 0);
        set_reg(base, PxIS, 0xffff_ffff);
        set_reg(base, PxSERR, reg(base, PxSERR));

        set_bits(base, PxCMD, CMD_ST | CMD_FRE);
        if !wait_for(10_000_000, || {
            let cmd = reg(base, PxCMD);
            cmd & CMD_RUNNING_EMULATOR == CMD_RUNNING_EMULATOR
                || cmd & CMD_RUNNING_SPEC == CMD_RUNNING_SPEC
        }) {
            let cmd = reg(base, PxCMD);
            let task = reg(base, PxTFD);
            let status = reg(base, PxSSTS);
            let error = reg(base, PxSERR);
            println!("    CMD={cmd:#x} TFD={task:#x} SSTS={status:#x} SERR={error:#x}");
            return Err("engines did not start");
        }

        let mut controller = Self {
            abar,
            port,
            sectors: 0,
            block_size: 512,
            lba48: false,
        };
        controller.identify()?;
        Ok(controller)
    }

    /// Issue one command through slot 0 and wait for it to retire.
    fn execute(&mut self, command: u8, write: bool, lba: u64, count: u16) -> Result<(), &'static str> {
        let base = self.abar + PORT_BASE + self.port * PORT_STRIDE;

        if !wait_for(10_000_000, || reg(base, PxCI) & 1 == 0) {
            return Err("command slot stayed busy");
        }

        let table = &raw mut COMMAND_TABLE as *mut u8;
        let buffer = &raw const TRANSFER_BUFFER as u64;
        let ctba = &raw const COMMAND_TABLE as u64;

        // 20 byte register FIS: type, port, command, features, the six LBA
        // bytes, two count bytes, and control.
        let fis: [u8; 16] = [
            FIS_H2D_REGISTER,
            0x80,
            command,
            0,
            lba as u8,
            (lba >> 8) as u8,
            (lba >> 16) as u8,
            0x40,
            (lba >> 24) as u8,
            (lba >> 32) as u8,
            (lba >> 40) as u8,
            0,
            (count & 0xff) as u8,
            (count >> 8) as u8,
            0,
            0,
        ];
        unsafe { core::ptr::copy_nonoverlapping(fis.as_ptr(), table, fis.len()) };

        // One region descriptor: address low and high, reserved, and the byte
        // count stored as one less than the count with the completion bit.
        let bytes = count as usize * self.block_size;
        write_u32(table, CMD_TBL_HDR, buffer as u32);
        write_u32(table, CMD_TBL_HDR + 4, (buffer >> 32) as u32);
        write_u32(table, CMD_TBL_HDR + 8, 0);
        write_u32(table, CMD_TBL_HDR + 12, (bytes as u32 - 1) | (1 << 31));

        // Command list slot 0: command FIS length 5 double words, one region
        // descriptor, write flag, then the command table address.
        let list = &raw mut COMMAND_LIST as *mut u8;
        write_u32(list, 0, 5 | (1 << 16) | ((write as u32) << 6));
        write_u32(list, 4, 0);
        write_u32(list, 8, ctba as u32);
        write_u32(list, 12, (ctba >> 32) as u32);

        set_reg(base, PxCI, 1);
        if !wait_for(100_000_000, || reg(base, PxCI) & 1 == 0) {
            set_reg(base, PxCI, 0);
            return Err("command timed out");
        }

        if reg(base, PxTFD) & TFD_ERR != 0 {
            return Err("drive reported an error");
        }

        Ok(())
    }

    fn identify(&mut self) -> Result<(), &'static str> {
        self.execute(ATA_IDENTIFY, false, 0, 1)?;

        let data: &[u8; 512] = unsafe { &*(&raw const TRANSFER_BUFFER as *const [u8; 512]) };
        let word = |n: usize| -> u16 { u16::from_le_bytes([data[n * 2], data[n * 2 + 1]]) };

        if word(49) & (1 << 9) == 0 {
            return Err("device does not report LBA support");
        }

        let count_28 = (word(60) as u32) | ((word(61) as u32) << 16);
        let count_48 = (word(100) as u64)
            | ((word(101) as u64) << 16)
            | ((word(102) as u64) << 32)
            | ((word(103) as u64) << 48);

        // Bit 10 of word 83 says whether 48 bit addressing is usable.
        self.lba48 = (word(83) & (1 << 10) != 0) && count_48 != 0;
        self.sectors = if self.lba48 { count_48 } else { count_28 as u64 };

        let logical = word(106) as u32;
        self.block_size = if logical == 0 { 512 } else { logical as usize * 512 };

        if self.sectors == 0 {
            return Err("device reports zero sectors");
        }

        println!(
            "  device: {} sectors, {} byte sectors, {} addressing",
            self.sectors,
            self.block_size,
            if self.lba48 { "48 bit" } else { "28 bit" }
        );
        Ok(())
    }

    /// Total size of the disk in bytes.
    pub fn size(&self) -> u64 {
        self.sectors * self.block_size as u64
    }

    /// Size of one sector in bytes.
    pub fn sector_size(&self) -> u64 {
        self.block_size as u64
    }

    /// Read a byte range from a byte offset in the disk.
    pub fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), &'static str> {
        let block = self.block_size;
        let mut done = 0;

        while done < buffer.len() {
            // Absolute position each time. Decomposing offset once and adding
            // done / block would not advance the sector until done reached a
            // whole block, breaking any range that crosses a boundary.
            let at = offset as usize + done;
            let mut scratch = [0u8; 512];
            self.read_one((at / block) as u64, &mut scratch)?;

            let start = at % block;
            let chunk = (block - start).min(buffer.len() - done);
            buffer[done..done + chunk].copy_from_slice(&scratch[start..start + chunk]);
            done += chunk;
        }

        Ok(())
    }

    /// Write a byte range, keeping the rest of each sector it touches.
    pub fn write_at(&mut self, offset: u64, buffer: &[u8]) -> Result<(), &'static str> {
        let block = self.block_size;
        let mut done = 0;

        while done < buffer.len() {
            let at = offset as usize + done;
            let sector = (at / block) as u64;
            let start = at % block;
            let chunk = (block - start).min(buffer.len() - done);

            let mut scratch = [0u8; 512];
            if chunk < block {
                self.read_one(sector, &mut scratch)?;
            }
            scratch[start..start + chunk].copy_from_slice(&buffer[done..done + chunk]);
            self.write_one(sector, &scratch)?;

            done += chunk;
        }

        Ok(())
    }

    fn read_one(&mut self, lba: u64, buffer: &mut [u8; 512]) -> Result<(), &'static str> {
        let command = if self.lba48 {
            ATA_READ_DMA_EXT
        } else {
            // 28 bit LBA puts the top four bits in the device register instead
            // of the extended LBA bytes.
            self.set_device_register(lba);
            0xC8
        };

        self.execute(command, false, lba, 1)?;
        let scratch = unsafe { &*(&raw const TRANSFER_BUFFER as *const [u8; 512]) };
        buffer.copy_from_slice(scratch);
        Ok(())
    }

    fn write_one(&mut self, lba: u64, buffer: &[u8; 512]) -> Result<(), &'static str> {
        let scratch = &raw mut TRANSFER_BUFFER as *mut [u8; 512];
        unsafe { scratch.copy_from_nonoverlapping(buffer, 1) };

        let command = if self.lba48 {
            ATA_WRITE_DMA_EXT
        } else {
            self.set_device_register(lba);
            0xCA
        };

        self.execute(command, true, lba, 1)
    }

    fn set_device_register(&mut self, lba: u64) {
        let table = &raw mut COMMAND_TABLE as *mut u8;
        unsafe { *table.add(7) = 0x40 | ((lba >> 24) & 0x0f) as u8 };
    }
}
