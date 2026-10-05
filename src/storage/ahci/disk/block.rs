use crate::storage::ahci::Disk;

pub(crate) trait BlockDevice: Sync + Send + 'static {
    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str>;
    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str>;
    fn sector_size(&self) -> usize;
}

impl BlockDevice for Disk {
    // convert lba to byte offset
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

// A partition addresses sectors from zero at its own first sector. This is what
// makes the filesystem's arithmetic correct: it computes offsets relative to the
// volume boot record, so the offset to the disk is added exactly once, here.
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
        // Inherent method, not this one -- say so, since the names match.
        super::partition::Partition::sector_size(self) as usize
    }
}
