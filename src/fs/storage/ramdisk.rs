use alloc::vec;
use alloc::vec::Vec;

use super::block::{BlockDevice, SECTOR_SIZE};

pub struct RamDisk {
    data: Vec<u8>,
}

impl RamDisk {
    pub fn new(sectors: usize) -> Self {
        Self {
            data: vec![0; sectors * SECTOR_SIZE],
        }
    }
}

impl BlockDevice for RamDisk {
    fn read_sector(
        &mut self,
        sector: u64,
        buffer: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), &'static str> {
        let start = sector as usize * SECTOR_SIZE;
        let end = start + SECTOR_SIZE;

        if end > self.data.len() {
            return Err("sector out of bounds");
        }

        buffer.copy_from_slice(&self.data[start..end]);

        Ok(())
    }

    fn write_sector(
        &mut self,
        sector: u64,
        buffer: &[u8; SECTOR_SIZE],
    ) -> Result<(), &'static str> {
        let start = sector as usize * SECTOR_SIZE;
        let end = start + SECTOR_SIZE;

        if end > self.data.len() {
            return Err("sector out of bounds");
        }

        self.data[start..end].copy_from_slice(buffer);

        Ok(())
    }
    fn sector_count(&self) -> u64 {
    (self.data.len() / SECTOR_SIZE) as u64
}

}