use alloc::{boxed::Box, vec::Vec};

use fatfs::{FileSystem, FsOptions, IoBase, Read, Seek, SeekFrom, Write};

use crate::arch::ahci::AhciController;
use crate::println;

const SECTOR: u64 = 512;

// blanket impl for block dev
pub trait BlockDevice: Read<Error = fatfs::Error<()>> + Write + Seek + Send {}
impl<T: Read<Error = fatfs::Error<()>> + Write + Seek + Send> BlockDevice for T {}

impl IoBase for Box<dyn BlockDevice> {
    type Error = fatfs::Error<()>;
}

impl Read for Box<dyn BlockDevice> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        (**self).read(buffer)
    }
}

impl Write for Box<dyn BlockDevice> {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        (**self).write(buffer)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        (**self).flush()
    }
}

impl Seek for Box<dyn BlockDevice> {
    fn seek(&mut self, from: SeekFrom) -> Result<u64, Self::Error> {
        (**self).seek(from)
    }
}

struct Volume {
    first: u64,
    sectors: u64,
}

pub struct Disk {
    controller: AhciController,
    base: u64,
    length: u64,
    position: u64,
}

impl Disk {
    fn new(controller: AhciController, volume: &Volume) -> Self {
        Self {
            controller,
            base: volume.first,
            length: volume.sectors * SECTOR,
            position: 0,
        }
    }

    fn at(&self) -> u64 {
        self.base * SECTOR + self.position
    }

    fn fits(&self, len: usize) -> bool {
        self.position + len as u64 <= self.length
    }
}

impl IoBase for Disk {
    type Error = fatfs::Error<()>;
}

impl Read for Disk {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        if !self.fits(buffer.len()) {
            return Err(fatfs::Error::Io(()));
        }

        let at = self.at();
        self.controller
            .read_at(at, buffer)
            .map_err(|_| fatfs::Error::Io(()))?;
        self.position += buffer.len() as u64;
        Ok(buffer.len())
    }
}

impl Write for Disk {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        if !self.fits(buffer.len()) {
            return Err(fatfs::Error::Io(()));
        }

        let at = self.at();
        self.controller
            .write_at(at, buffer)
            .map_err(|_| fatfs::Error::Io(()))?;
        self.position += buffer.len() as u64;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Seek for Disk {
    fn seek(&mut self, from: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let target = match from {
            fatfs::SeekFrom::Start(at) => at as i64,
            fatfs::SeekFrom::End(at) => self.length as i64 + at,
            fatfs::SeekFrom::Current(at) => self.position as i64 + at,
        };

        if target < 0 || target as u64 > self.length {
            return Err(fatfs::Error::Io(()));
        }

        self.position = target as u64;
        Ok(self.position)
    }
}

/// `0x55AA` alone is a false positive: sector 0 of a partitioned disk carries
/// it with nothing behind it. The type string is what identifies a volume.
fn is_volume(sector: &[u8]) -> bool {
    let bytes_per_sector = u16::from_le_bytes([sector[0x0b], sector[0x0c]]);
    let signature = u16::from_le_bytes([sector[510], sector[511]]);

    bytes_per_sector == SECTOR as u16 && signature == 0xaa55 && &sector[0x52..0x55] == b"FAT"
}

fn from_partition_table(disk: &mut AhciController) -> Vec<Volume> {
    let mut out = Vec::new();
    let mut header = [0u8; 512];

    if disk.read_at(SECTOR, &mut header).is_err() || &header[..8] != b"EFI PART" {
        return out;
    }

    let table = u64::from_le_bytes(header[72..80].try_into().unwrap());
    let count = u32::from_le_bytes(header[80..84].try_into().unwrap()) as usize;
    let size = u32::from_le_bytes(header[84..88].try_into().unwrap()) as usize;

    if size < 128 || count == 0 || count > 256 {
        return out;
    }

    for index in 0..count {
        let mut entry = [0u8; 128];
        let at = (table + index as u64) * SECTOR;

        if disk.read_at(at, &mut entry).is_err() {
            break;
        }
        if entry[..16] == [0u8; 16] {
            continue;
        }

        let first = u64::from_le_bytes(entry[32..40].try_into().unwrap());
        let last = u64::from_le_bytes(entry[40..48].try_into().unwrap());

        if first != 0 && last >= first {
            out.push(Volume {
                first,
                sectors: last - first + 1,
            });
        }
    }

    out
}

/// Done before mounting, because mounting consumes the controller and this
/// borrows it.
fn find_volume(disk: &mut AhciController) -> Option<Volume> {
    for volume in from_partition_table(disk) {
        let mut sector = [0u8; 512];
        if disk.read_at(volume.first * SECTOR, &mut sector).is_ok() && is_volume(&sector) {
            return Some(volume);
        }
    }

    None
}

pub fn mount(
    mut controller: AhciController,
) -> Result<FileSystem<Box<dyn BlockDevice>>, &'static str> {
    let volume = find_volume(&mut controller).ok_or("no FAT volume found")?;
    println!(
        "volume at sector {} ({} sectors, byte {:#x})",
        volume.first,
        volume.sectors,
        volume.first * SECTOR
    );

    let disk: Box<dyn BlockDevice> = Box::new(Disk::new(controller, &volume));

    FileSystem::new(disk, FsOptions::new()).map_err(|_| "volume is not FAT")
}
