use crate::storage::ahci::Disk;

pub trait BlockDevice: Sync + Send + 'static {
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
