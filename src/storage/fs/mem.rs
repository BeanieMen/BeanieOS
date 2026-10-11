use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use spin::Mutex;

use crate::storage::ahci::disk::block::BlockDevice;
use crate::storage::vfs::dentry::Dentry;
use crate::storage::vfs::inode::{FileOperations, FileType, Inode, InodeOperations, RawEntry};

pub(crate) const SECTOR: usize = 512;
pub(crate) const SECTORS: usize = 4096;
pub(crate) const FAT_START: u64 = 32;
pub(crate) const FAT_SECTORS: u32 = 128;
pub(crate) const FAT_COUNT: u8 = 1;
pub(crate) const RESERVED_SECTORS: u16 = 32;
pub(crate) const SECTORS_PER_CLUSTER: u8 = 4;
pub(crate) const ROOT_CLUSTER: u32 = 2;

/// Placeholder operations: this device only supplies sectors, so the filesystem
/// layer above it is what does the actual parsing.
struct Unsupported;

impl InodeOperations for Unsupported {
    fn lookup(&self, _: &Inode, _: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("no lookup on the memory device")
    }

    fn create(
        &self,
        _: &mut Inode,
        _: &[u8],
        _: FileType,
    ) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("no create on the memory device")
    }

    fn unlink(&self, _: &mut Inode, _: &[u8]) -> Result<(), &'static str> {
        Err("no unlink on the memory device")
    }

    fn mkdir(&self, _: &mut Inode, _: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        Err("no mkdir on the memory device")
    }

    fn rmdir(&self, _: &mut Inode, _: &[u8]) -> Result<(), &'static str> {
        Err("no rmdir on the memory device")
    }
}

impl FileOperations for Unsupported {
    fn read(&self, _: &Inode, _: u64, _: &mut [u8]) -> Result<usize, &'static str> {
        Err("no read on the memory device")
    }

    fn write(&self, _: &mut Inode, _: u64, _: &[u8]) -> Result<usize, &'static str> {
        Err("no write on the memory device")
    }

    fn readdir(&self, _: &Inode, _: u64) -> Result<Option<RawEntry>, &'static str> {
        Err("no readdir on the memory device")
    }
}

static UNSUPPORTED: Unsupported = Unsupported;

pub(crate) struct MemDevice {
    data: Vec<u8>,
    root: Arc<Mutex<Dentry>>,
}

impl MemDevice {
    pub(crate) fn new() -> Self {
        let mut device = MemDevice {
            data: vec![0u8; SECTOR * SECTORS],
            root: Arc::new(Mutex::new(Dentry::new(
                alloc::string::String::from("/"),
                Arc::new(Mutex::new(Inode {
                    id: ROOT_CLUSTER as u64,
                    file_type: FileType::Directory,
                    size: 0,
                    cluster: ROOT_CLUSTER,
                    inode_ops: &UNSUPPORTED,
                    file_ops: &UNSUPPORTED,
                    dentry: None,
                })),
                None,
            ))),
        };

        device.format();
        device
    }

    fn put(&mut self, lba: u64, bytes: &[u8]) {
        let at = lba as usize * SECTOR;
        self.data[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn fat_set(&mut self, cluster: u32, value: u32) {
        let sector = FAT_START + (cluster as u64 * 4) / SECTOR as u64;
        let off = ((cluster as u64 * 4) % SECTOR as u64) as usize;

        let mut entry = [0u8; 4];
        entry.copy_from_slice(&self.data[sector as usize * SECTOR + off..][..4]);

        let current = u32::from_le_bytes(entry);
        let updated = (current & 0xFFFF_0000) | (value & 0x0FFF_FFFF);

        self.data[sector as usize * SECTOR + off..][..4]
            .copy_from_slice(&updated.to_le_bytes());
    }

    fn format(&mut self) {
        let mut boot = [0u8; SECTOR];

        boot[0] = 0xEB;
        boot[1] = 0x58;
        boot[2] = 0x90;
        boot[3..11].copy_from_slice(b"MSDOS5.0");
        boot[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        boot[13] = SECTORS_PER_CLUSTER;
        boot[14..16].copy_from_slice(&RESERVED_SECTORS.to_le_bytes());
        boot[16] = FAT_COUNT;
        boot[17..19].copy_from_slice(&0u16.to_le_bytes());
        boot[19..21].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        boot[21..23].copy_from_slice(&((SECTORS_PER_CLUSTER as u16) * 2).to_le_bytes());
        boot[22] = 0;
        boot[23] = 0xF8;
        boot[24..26].copy_from_slice(&(32u16).to_le_bytes());
        boot[26..28].copy_from_slice(&(1u16).to_le_bytes());
        boot[28..30].copy_from_slice(&(ROOT_CLUSTER as u16).to_le_bytes());
        boot[32..34].copy_from_slice(&(ROOT_CLUSTER as u16).to_le_bytes());
        boot[36..40].copy_from_slice(&FAT_SECTORS.to_le_bytes());
        boot[40..42].copy_from_slice(&0u16.to_le_bytes());
        boot[42..44].copy_from_slice(&(ROOT_CLUSTER as u16).to_le_bytes());
        boot[44..48].copy_from_slice(&(ROOT_CLUSTER as u32).to_le_bytes());
        boot[64] = 0x80;
        boot[66] = 0x29;
        boot[510] = 0x55;
        boot[511] = 0xAA;

        self.put(0, &boot);

        self.fat_set(0, 0x0FFF_FFF8);
        self.fat_set(1, 0x0FFF_FFFF);
        self.fat_set(ROOT_CLUSTER, 0x0FFF_FFFF);
    }
}

impl BlockDevice for MemDevice {
    fn read_block(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        if (lba as usize + 1) * SECTOR > self.data.len() {
            return Err("read past the end of the device");
        }

        let len = buf.len().min(SECTOR);
        buf[..len].copy_from_slice(&self.data[lba as usize * SECTOR..][..len]);
        Ok(())
    }

    fn write_block(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str> {
        if (lba as usize + 1) * SECTOR > self.data.len() {
            return Err("write past the end of the device");
        }

        let len = buf.len().min(SECTOR);
        self.data[lba as usize * SECTOR..][..len].copy_from_slice(&buf[..len]);
        Ok(())
    }

    fn sector_size(&self) -> usize {
        SECTOR
    }
}