pub const SECTOR_SIZE: usize = 512;

pub trait BlockDevice {
    fn sector_count(&self) -> u64;

    fn read_sector(
        &mut self,
        sector: u64,
        buffer: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), &'static str>;

    fn write_sector(
        &mut self,
        sector: u64,
        buffer: &[u8; SECTOR_SIZE],
    ) -> Result<(), &'static str>;
}