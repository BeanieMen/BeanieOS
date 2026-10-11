use super::disk::Disk;

pub(crate) trait BlockDevice: Sync + Send + 'static {
    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str>;
    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str>;
    fn sector_size(&self) -> usize;
}

impl BlockDevice for Disk {
    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        self.read_at(lba * 512, buf)
    }
    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str> {
        self.write_at(lba * 512, buf)
    }
    fn sector_size(&self) -> usize {
        self.block_size
    }
}

impl BlockDevice for super::partition::Partition {
    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let at = lba
            .checked_mul(self.sector_size())
            .ok_or("sector offset overflows")?;

        self.read_at(at, buf)
    }

    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str> {
        let at = lba
            .checked_mul(self.sector_size())
            .ok_or("sector offset overflows")?;

        self.write_at(at, buf)
    }

    fn sector_size(&self) -> usize {
        super::partition::Partition::sector_size(self) as usize
    }
}
