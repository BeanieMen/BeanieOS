use simple_ahci::AhciDriver;

use crate::arch::ahci::Hal;
use crate::fs::storage::block::{BlockDevice, SECTOR_SIZE};

pub struct AhciDisk {
    driver: AhciDriver<Hal>,
}

impl AhciDisk {
    pub fn new(driver: AhciDriver<Hal>) -> Self {
        Self { driver }
    }
}


impl BlockDevice for AhciDisk {
    fn sector_count(&self) -> u64 {
        self.driver.capacity()
    }

    fn read_sector(
        &mut self,
        sector: u64,
        buffer: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), &'static str> {
        if self.driver.block_size() != SECTOR_SIZE {
            return Err("AHCI block size is not 512 bytes");
        }

        if self.driver.read(sector, buffer) {
            Ok(())
        } else {
            Err("AHCI read failed")
        }
    }

    fn write_sector(
        &mut self,
        sector: u64,
        buffer: &[u8; SECTOR_SIZE],
    ) -> Result<(), &'static str> {
        if self.driver.block_size() != SECTOR_SIZE {
            return Err("AHCI block size is not 512 bytes");
        }

        if self.driver.write(sector, buffer) {
            Ok(())
        } else {
            Err("AHCI write failed")
        }
    }
}
