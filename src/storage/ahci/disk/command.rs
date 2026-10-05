use super::super::regs::*;
use super::disk::Disk;
use crate::hal::dma::{CMD_TBL_HDR, PRD_COUNT, PRD_SIZE};

// Command table header: region table length, then the table's own address.
const PRDT_LENGTH_OFFSET: usize = 0x46;
const CMD_TABLE_BASE_OFFSET: usize = 0x48;

impl Disk {
    pub(super) fn execute(
        &mut self,
        command: u8,
        write: bool,
        lba: u64,
        count: u16,
    ) -> Result<(), &'static str> {
        let base = self.port_base();

        if reg(base, PX_CI) & 1 != 0 {
            return Err("command slot still busy");
        }

        let table = self.dma.command_table() as *mut u8;
        let buffer = self.transfer_buffer() as u64;
        let ctba = self.dma.command_table() as u64;

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

        write_u32(table, PRDT_LENGTH_OFFSET, PRD_SIZE);
        write_u32(table, CMD_TABLE_BASE_OFFSET, ctba as u32);
        write_u32(table, CMD_TABLE_BASE_OFFSET + 4, (ctba >> 32) as u32);

        // One region descriptor: address, reserved, byte count as count - 1
        // with the completion bit set.
        let bytes = count as usize * self.block_size;
        write_u32(table, CMD_TBL_HDR, buffer as u32);
        write_u32(table, CMD_TBL_HDR + 4, (buffer >> 32) as u32);
        write_u32(table, CMD_TBL_HDR + 8, 0);
        write_u32(table, CMD_TBL_HDR + 12, (bytes as u32 - 1) | (1 << 31));

        // Slot 0: FIS length in the low byte, then the command table address.
        // The descriptor count goes in bits 16-31, where the emulator looks for
        // it; the spec puts the write and prefetch flags there instead. Without
        // a count the region table is never walked and nothing transfers.
        let list = self.dma.command_list() as *mut u8;
        write_u32(list, 0, 5 | (PRD_COUNT << 16) | ((write as u32) << 6));
        write_u32(list, 4, 0);
        write_u32(list, 8, ctba as u32);
        write_u32(list, 12, (ctba >> 32) as u32);

        // Armed first, so an instant completion cannot beat the clear.
        super::super::irq::arm(base);
        set_reg(base, PX_CI, 1);

        super::super::irq::wait(base)?;

        // A host error lands in PxIS with PxTFD clean, so read both.
        let status = reg(base, PX_IS);

        if status & ERROR_BITS != 0 || reg(base, PX_TFD) & TFD_ERR != 0 {
            return Err(match take_signature_error(base) {
                Some(why) => why,
                None => "drive reported an error",
            });
        }

        Ok(())
    }
}
