//! FAT32 over the AHCI driver.
//!
//! The volume is discovered at runtime rather than assumed. Partition table
//! entries are tried first, then mebibyte-aligned offsets, because a volume
//! does not need a partition entry to be readable. Nothing here assumes a
//! partition type, a table layout, or a particular offset.

use alloc::vec::Vec;

use fatfs::{FileSystem, FsOptions, IoBase, Read, Seek, Write};

use crate::arch::ahci::AhciController;
use crate::println;

const SECTOR: u64 = 512;
const SCAN_STEP: u64 = 1024 * 1024;

pub struct Disk {
    controller: AhciController,
    base: u64,
    position: u64,
}

impl Disk {
    fn new(controller: AhciController, base: u64) -> Self {
        Self {
            controller,
            base,
            position: 0,
        }
    }

    fn at(&self) -> u64 {
        self.base * SECTOR + self.position
    }
}

impl IoBase for Disk {
    type Error = fatfs::Error<()>;
}

impl Read for Disk {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
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
            fatfs::SeekFrom::End(at) => self.controller.size() as i64 + at,
            fatfs::SeekFrom::Current(at) => self.position as i64 + at,
        };

        if target < 0 {
            return Err(fatfs::Error::Io(()));
        }

        self.position = target as u64;
        Ok(self.position)
    }
}

/// Does this sector start a FAT volume?
///
/// The signature alone is not enough: sector 0 of a partitioned disk carries
/// `0x55AA` with nothing behind it, and would be a false positive. The file
/// system type string is what actually identifies a volume.
fn is_volume(sector: &[u8]) -> bool {
    let bytes_per_sector = u16::from_le_bytes([sector[0x0b], sector[0x0c]]);
    let signature = u16::from_le_bytes([sector[510], sector[511]]);

    bytes_per_sector == SECTOR as u16 && signature == 0xaa55 && &sector[0x52..0x55] == b"FAT"
}

/// Start sectors named by a partition table, if there is one.
fn from_partition_table(disk: &mut AhciController) -> Vec<u64> {
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
        if first != 0 {
            out.push(first);
        }
    }

    out
}

/// Every mebibyte-aligned offset in the disk.
fn by_alignment(disk: &AhciController) -> Vec<u64> {
    let mut out = Vec::new();
    let mut at = SCAN_STEP;

    while at < disk.size() / SECTOR {
        out.push(at);
        at += SCAN_STEP;
    }

    out
}

/// Find the first sector that starts a volume. Done before mounting, because
/// mounting consumes the controller and this borrows it.
fn find_volume(disk: &mut AhciController) -> Option<u64> {
    let mut candidates = from_partition_table(disk);

    for at in by_alignment(disk) {
        if !candidates.contains(&at) {
            candidates.push(at);
        }
    }

    for first in candidates {
        let mut sector = [0u8; 512];
        if disk.read_at(first * SECTOR, &mut sector).is_ok() && is_volume(&sector) {
            return Some(first);
        }
    }

    None
}

pub fn mount(mut controller: AhciController) -> Result<FileSystem<Disk>, &'static str> {
    let first = find_volume(&mut controller).ok_or("no FAT volume found")?;
    println!("volume at sector {first} (byte {:#x})", first * SECTOR);

    FileSystem::new(Disk::new(controller, first), FsOptions::new()).map_err(|_| "volume is not FAT")
}
