mod command;
mod table;

use crate::hal::dma::{AhciDma, BUF_SIZE, TransferBuffer};
use super::regs::*;
use crate::{kdebug, kerror};

// IDENTIFY strings: words 10-19 and 27-46, bytes stored swapped.
const SERIAL_WORD: usize = 10;
const SERIAL_LEN: usize = 20;
const MODEL_WORD: usize = 27;
const MODEL_LEN: usize = 40;

fn ata_string(data: &[u8; 512], first_word: usize, out: &mut [u8]) {
    for i in 0..out.len() / 2 {
        out[i * 2] = data[(first_word + i) * 2 + 1];
        out[i * 2 + 1] = data[(first_word + i) * 2];
    }
}

fn text(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());

    core::str::from_utf8(&bytes[..end]).unwrap_or("?").trim()
}

/// Copyable: a command needs only the MMIO base and port index. All disks
/// share one `DMA` block, so one transfer at a time.
#[derive(Clone, Copy)]
pub struct Disk {
    dma: &'static AhciDma,
    abar: usize,
    port: usize,
    sectors: u64,
    block_size: usize,
    lba48: bool,
    model: [u8; MODEL_LEN],
    serial: [u8; SERIAL_LEN],
}

impl Disk {
    pub(super) fn attach(
        dma: &'static AhciDma,
        abar: usize,
        port: usize,
    ) -> Result<Self, &'static str> {
        let base = abar + PORT_BASE + port * PORT_STRIDE;

        // The spec allows 500 ms for a port reset; the emulator takes longer.
        set_reg(base, PX_CMD, 0);
        if !wait_for(1000, || reg(base, PX_CMD) & CMD_RUNNING_ANY == 0) {
            return Err("did not stop");
        }

        // Identity mapped, so these are the physical addresses.
        let clb = dma.command_list() as u64;
        let fb = dma.receive_area() as u64;
        set_reg(base, PX_CLB, clb as u32);
        set_reg(base, PX_CLBU, (clb >> 32) as u32);
        set_reg(base, PX_FB, fb as u32);
        set_reg(base, PX_FBU, (fb >> 32) as u32);

        set_bits(base, PX_CMD, CMD_SRE);

        // PxSSTS.DET is 0 on an empty port, so one read rules it out first.
        if reg(base, PX_SSTS) & SSTS_DET_MASK == 0 {
            return Err("no device");
        }

        // Spin-up is read-only in the emulator, so the link is the only evidence.
        if !wait_for(5000, || {
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
        if !wait_for(1000, || {
            let cmd = reg(base, PX_CMD);
            cmd & CMD_RUNNING_EMULATOR == CMD_RUNNING_EMULATOR
                || cmd & CMD_RUNNING_SPEC == CMD_RUNNING_SPEC
        }) {
            let cmd = reg(base, PX_CMD);
            let task = reg(base, PX_TFD);
            let status = reg(base, PX_SSTS);
            let error = reg(base, PX_SERR);
            kerror!("CMD={cmd:#x} TFD={task:#x} SSTS={status:#x} SERR={error:#x}");
            return Err("engines did not start");
        }

        let mut disk = Disk {
            dma,
            abar,
            port,
            sectors: 0,
            block_size: SECTOR as usize,
            lba48: false,
            model: [0; MODEL_LEN],
            serial: [0; SERIAL_LEN],
        };

        disk.identify()?;

        Ok(disk)
    }

    fn port_base(&self) -> usize {
        self.abar + PORT_BASE + self.port * PORT_STRIDE
    }

    fn transfer_buffer(&self) -> *mut TransferBuffer {
        self.dma.transfer_buffer()
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

        // Word 83 bit 10 says 48 bit addressing is usable.
        self.lba48 = (word(83) & (1 << 10) != 0) && count_48 != 0;
        self.sectors = if self.lba48 {
            count_48
        } else {
            count_28 as u64
        };

        let block_size = if word(106) & 0xd000 == 0x5000 {
            (((word(118) as usize) << 16) | word(117) as usize) * 2
        } else {
            SECTOR as usize
        };
        if block_size != SECTOR as usize {
            return Err("only 512 byte logical sectors are supported");
        }
        self.block_size = block_size;

        if self.sectors == 0 {
            return Err("device reports zero sectors");
        }

        ata_string(data, SERIAL_WORD, &mut self.serial);
        ata_string(data, MODEL_WORD, &mut self.model);

        kdebug!(
            "    identified: {} sectors, {} byte sectors, {} addressing",
            self.sectors,
            self.block_size,
            if self.lba48 { "48 bit" } else { "28 bit" }
        );

        Ok(())
    }

    pub fn port(&self) -> usize {
        self.port
    }

    pub fn model(&self) -> &str {
        text(&self.model)
    }

    pub fn serial(&self) -> &str {
        text(&self.serial)
    }

    pub fn sectors(&self) -> u64 {
        self.sectors
    }

    pub fn sector_size(&self) -> u64 {
        self.block_size as u64
    }

    pub fn size(&self) -> u64 {
        self.sectors * self.block_size as u64
    }

    pub fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), &'static str> {
        let mut done = 0;

        while done < buffer.len() {
            let at = offset as usize + done;
            let (sector, start) = self.locate(at)?;
            let count = self.batch(start, buffer.len() - done)?;
            self.read_run(sector, count)?;

            let fresh = unsafe { &*(self.transfer_buffer() as *const [u8; BUF_SIZE]) };
            let chunk = (self.block_size * count as usize - start).min(buffer.len() - done);
            buffer[done..done + chunk].copy_from_slice(&fresh[start..start + chunk]);
            done += chunk;
        }

        Ok(())
    }

    pub fn write_at(&mut self, offset: u64, buffer: &[u8]) -> Result<(), &'static str> {
        let mut done = 0;

        while done < buffer.len() {
            let at = offset as usize + done;
            let (sector, start) = self.locate(at)?;
            let count = self.batch(start, buffer.len() - done)?;
            let span = self.block_size * count as usize;

            // A partial write must read first or it zeroes the rest.
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
