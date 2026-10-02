use core::cell::UnsafeCell;
use core::ptr::{read_volatile, write_volatile};

use crate::hal::pci::Device;
use crate::println;

const CAP: usize = 0x00;
const GHC: usize = 0x04;
const PI: usize = 0x0c;
const VS: usize = 0x10;

const PORT_BASE: usize = 0x100;
const PORT_STRIDE: usize = 0x80;

const PX_CLB: usize = 0x00;
const PX_CLBU: usize = 0x04;
const PX_FB: usize = 0x08;
const PX_FBU: usize = 0x0c;
const PX_IS: usize = 0x10;
const PX_IE: usize = 0x14;
const PX_CMD: usize = 0x18;
const PX_TFD: usize = 0x20;
const PX_SSTS: usize = 0x28;
const PX_SERR: usize = 0x30;
const PX_CI: usize = 0x38;

const GHC_HR: u32 = 1 << 0;
const GHC_AE: u32 = 1 << 1;

// 0 ST, 1 SRE, 4 FRE. 2, 3, 14 and 15 are read-only: the spec reports the
// engines running on 2 and 3, the emulator on 14 and 15, so either counts.
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

#[repr(C, align(4096))]
struct Dma {
    command_list: UnsafeCell<CommandList>,
    command_table: UnsafeCell<CommandTable>,
    receive_area: UnsafeCell<ReceiveArea>,
    transfer_buffer: UnsafeCell<TransferBuffer>,
}

unsafe impl Sync for Dma {}

static DMA: Dma = Dma {
    command_list: UnsafeCell::new(CommandList([0; CLB_SIZE])),
    command_table: UnsafeCell::new(CommandTable([0; CMD_TBL_HDR + 16])),
    receive_area: UnsafeCell::new(ReceiveArea([0; FB_SIZE])),
    transfer_buffer: UnsafeCell::new(TransferBuffer([0; BUF_SIZE])),
};

pub struct AhciController {
    dma: &'static Dma,
    abar: usize,
    port: usize,
    sectors: u64,
    block_size: usize,
    lba48: bool,
}

impl AhciController {
    fn command_list(&self) -> *mut CommandList {
        self.dma.command_list.get()
    }

    fn command_table(&self) -> *mut CommandTable {
        self.dma.command_table.get()
    }

    fn transfer_buffer(&self) -> *mut TransferBuffer {
        self.dma.transfer_buffer.get()
    }
}

fn take_signature_error(base: usize) -> Option<&'static str> {
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

    println!("    PxSERR={raw:#x}: {named}");

    Some(named)
}

fn reg(base: usize, offset: usize) -> u32 {
    unsafe { read_volatile((base + offset) as *const u32) }
}

fn set_reg(base: usize, offset: usize, value: u32) {
    unsafe { write_volatile((base + offset) as *mut u32, value) }
}

fn set_bits(base: usize, offset: usize, bits: u32) {
    set_reg(base, offset, reg(base, offset) | bits);
}

fn write_u32(buffer: *mut u8, offset: usize, value: u32) {
    unsafe { write_volatile(buffer.add(offset) as *mut u32, value) }
}

/// Iterations, not milliseconds, so this needs no timer and a missing device
/// cannot hang the kernel.
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
        let (phys, _) = device.bar5_info().ok_or("no BAR5")?;
        let abar = crate::hal::hal()
            .mmio
            .mapping_at(crate::hal::mmio::MMIO_BASE as usize + phys as usize)
            .map(|m| m.va)
            .unwrap_or(crate::hal::mmio::MMIO_BASE as usize + phys as usize);

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
            (DMA.command_list.get() as *mut u8, CLB_SIZE),
            (DMA.command_table.get() as *mut u8, CMD_TBL_HDR + 16),
            (DMA.receive_area.get() as *mut u8, FB_SIZE),
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

        set_reg(base, PX_CMD, 0);
        if !wait_for(10_000_000, || reg(base, PX_CMD) & CMD_RUNNING_ANY == 0) {
            return Err("did not stop");
        }

        // Identity mapped, so these addresses are the physical addresses.
        let clb = DMA.command_list.get() as u64;
        let fb = DMA.receive_area.get() as u64;
        set_reg(base, PX_CLB, clb as u32);
        set_reg(base, PX_CLBU, (clb >> 32) as u32);
        set_reg(base, PX_FB, fb as u32);
        set_reg(base, PX_FBU, (fb >> 32) as u32);

        set_bits(base, PX_CMD, CMD_SRE);

        // The staggered spin-up bit is read-only in the emulator, so the link
        // is the only usable evidence. 3 means present and physical layer up.
        if !wait_for(50_000_000, || {
            reg(base, PX_SSTS) & SSTS_DET_MASK == SSTS_DET_PHY_UP
        }) {
            return Err(match reg(base, PX_SSTS) & SSTS_DET_MASK {
                0 => "no device",
                _ => "link did not come up",
            });
        }

        set_reg(base, PX_IE, 0);
        set_reg(base, PX_IS, 0xffff_ffff);
        set_reg(base, PX_SERR, reg(base, PX_SERR));

        set_bits(base, PX_CMD, CMD_ST | CMD_FRE);
        if !wait_for(10_000_000, || {
            let cmd = reg(base, PX_CMD);
            cmd & CMD_RUNNING_EMULATOR == CMD_RUNNING_EMULATOR
                || cmd & CMD_RUNNING_SPEC == CMD_RUNNING_SPEC
        }) {
            let cmd = reg(base, PX_CMD);
            let task = reg(base, PX_TFD);
            let status = reg(base, PX_SSTS);
            let error = reg(base, PX_SERR);
            println!("    CMD={cmd:#x} TFD={task:#x} SSTS={status:#x} SERR={error:#x}");
            return Err("engines did not start");
        }

        let mut controller = Self {
            dma: &DMA,
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
    fn execute(
        &mut self,
        command: u8,
        write: bool,
        lba: u64,
        count: u16,
    ) -> Result<(), &'static str> {
        let base = self.abar + PORT_BASE + self.port * PORT_STRIDE;

        if !wait_for(10_000_000, || reg(base, PX_CI) & 1 == 0) {
            return Err("command slot stayed busy");
        }

        let table = self.command_table() as *mut u8;
        let buffer = self.transfer_buffer() as u64;
        let ctba = self.command_table() as u64;

        let device = if self.lba48 {
            0x40
        } else {
            0x40 | ((lba >> 24) & 0x0f) as u8
        };

        let fis: [u8; 16] = [
            FIS_H2D_REGISTER,
            0x80,
            command,
            0,
            lba as u8,
            (lba >> 8) as u8,
            (lba >> 16) as u8,
            device,
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
        let list = self.command_list() as *mut u8;
        write_u32(list, 0, 5 | (1 << 16) | ((write as u32) << 6));
        write_u32(list, 4, 0);
        write_u32(list, 8, ctba as u32);
        write_u32(list, 12, (ctba >> 32) as u32);

        set_reg(base, PX_CI, 1);
        if !wait_for(100_000_000, || reg(base, PX_CI) & 1 == 0) {
            set_reg(base, PX_CI, 0);

            return Err(match take_signature_error(base) {
                Some(why) => why,
                None => "command timed out",
            });
        }

        if reg(base, PX_TFD) & TFD_ERR != 0 {
            return Err(match take_signature_error(base) {
                Some(why) => why,
                None => "drive reported an error",
            });
        }

        Ok(())
    }

    fn identify(&mut self) -> Result<(), &'static str> {
        self.execute(ATA_IDENTIFY, false, 0, 1)?;

        let data: &[u8; 512] = unsafe { &*(self.transfer_buffer() as *const [u8; 512]) };
        let word = |n: usize| -> u16 { u16::from_le_bytes([data[n * 2], data[n * 2 + 1]]) };

        if word(49) & (1 << 9) == 0 {
            return Err("device does not report LBA support");
        }

        if word(83) & 0xc000 != 0x4000 {
            return Err("device does not report a valid command set");
        }

        let count_28 = (word(60) as u32) | ((word(61) as u32) << 16);
        let count_48 = (word(100) as u64)
            | ((word(101) as u64) << 16)
            | ((word(102) as u64) << 32)
            | ((word(103) as u64) << 48);

        // Bit 10 of word 83 says whether 48 bit addressing is usable.
        self.lba48 = (word(83) & (1 << 10) != 0) && count_48 != 0;
        self.sectors = if self.lba48 {
            count_48
        } else {
            count_28 as u64
        };

        let block_size = if word(106) & 0xd000 == 0x5000 {
            (((word(118) as usize) << 16) | word(117) as usize) * 2
        } else {
            512
        };
        if block_size != 512 {
            return Err("only 512 byte logical sectors are supported");
        }
        self.block_size = block_size;

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

    pub fn size(&self) -> u64 {
        self.sectors * self.block_size as u64
    }

    pub fn sector_size(&self) -> u64 {
        self.block_size as u64
    }

    pub fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), &'static str> {
        let mut done = 0;

        while done < buffer.len() {
            let at = offset as usize + done;
            let (sector, start) = self.locate(at)?;
            let count = self.batch(start, buffer.len() - done)?;
            self.read_run(sector, count)?;

            // Straight out of the transfer buffer the device just filled,
            // instead of a second copy through a scratch array.
            let fresh = unsafe { &*(self.transfer_buffer() as *const [u8; BUF_SIZE]) };
            let chunk = (self.block_size * count as usize - start).min(buffer.len() - done);
            buffer[done..done + chunk].copy_from_slice(&fresh[start..start + chunk]);
            done += chunk;
        }

        Ok(())
    }

    /// Keeps the rest of each sector it touches.
    pub fn write_at(&mut self, offset: u64, buffer: &[u8]) -> Result<(), &'static str> {
        let mut done = 0;

        while done < buffer.len() {
            let at = offset as usize + done;
            let (sector, start) = self.locate(at)?;
            let count = self.batch(start, buffer.len() - done)?;
            let span = self.block_size * count as usize;

            // A whole-sector write needs no read first. A partial one has to
            // pick up the surrounding bytes or the write would zero them.
            if start != 0 || buffer.len() - done < span {
                self.read_run(sector, count)?;
            }

            let fresh = unsafe { &mut *(self.transfer_buffer() as *mut [u8; BUF_SIZE]) };
            let chunk = (span - start).min(buffer.len() - done);
            fresh[start..start + chunk].copy_from_slice(&buffer[done..done + chunk]);
            self.write_run(sector, count)?;

            done += chunk;
        }

        Ok(())
    }

    fn locate(&self, at: usize) -> Result<(u64, usize), &'static str> {
        let block = self.block_size;
        let sector = (at / block) as u64;

        if sector >= self.sectors {
            return Err("offset is past the end of the disk");
        }

        Ok((sector, at % block))
    }

    /// Sectors one command should move: enough to cover `want` bytes from
    /// `start`, bounded by the transfer buffer and by the disk.
    fn batch(&self, start: usize, want: usize) -> Result<u16, &'static str> {
        let needed = (want + start).div_ceil(self.block_size);
        let room = BUF_SIZE / self.block_size;

        Ok(needed.min(room).max(1) as u16)
    }

    fn read_run(&mut self, lba: u64, count: u16) -> Result<(), &'static str> {
        let command = if self.lba48 { ATA_READ_DMA_EXT } else { 0xC8 };

        self.execute(command, false, lba, count)
    }

    fn write_run(&mut self, lba: u64, count: u16) -> Result<(), &'static str> {
        let command = if self.lba48 { ATA_WRITE_DMA_EXT } else { 0xCA };

        self.execute(command, true, lba, count)
    }
}
