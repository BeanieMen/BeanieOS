use alloc::vec::Vec;

use super::super::regs::SECTOR;
use super::disk::Disk;
use super::partition::Partition;
use crate::{kdebug, kwarn};

impl Disk {
    // GPT only. No table yields an empty list, not an error.
    pub(crate) fn partitions(&mut self) -> Vec<Partition> {
        let mut out = Vec::new();
        let mut header = [0u8; SECTOR as usize];

        if self.read_at(SECTOR, &mut header).is_err() || &header[..8] != b"EFI PART" {
            kdebug!("disk {} carries no GPT", self.port);
            return out;
        }

        let table = u64::from_le_bytes(header[72..80].try_into().unwrap());
        let count = u32::from_le_bytes(header[80..84].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(header[84..88].try_into().unwrap()) as usize;

        if size < 128 || count == 0 || count > 256 {
            kwarn!("disk {} has an implausible GPT", self.port);
            return out;
        }

        // Entries are `size` bytes each, packed from the array's first LBA --
        // four 128-byte entries share one sector. Stepping a whole sector per
        // index lands on entry 4 * index and hides three partitions in four.
        let Some(base) = table.checked_mul(SECTOR) else {
            kwarn!("disk {}: GPT entry array LBA overflows", self.port);
            return out;
        };

        let end = base.saturating_add(count as u64 * size as u64);

        if end > self.size() {
            kwarn!(
                "disk {}: GPT entries end at {end}, past the {}-byte disk",
                self.port,
                self.size()
            );
            return out;
        }

        for index in 0..count {
            let mut entry = [0u8; 128];
            let at = base + index as u64 * size as u64;

            if self.read_at(at, &mut entry).is_err() {
                kwarn!("disk {}: entry {index} at byte {at} read failed", self.port);
                break;
            }
            if entry[..16] == [0u8; 16] {
                continue;
            }

            let first = u64::from_le_bytes(entry[32..40].try_into().unwrap());
            let last = u64::from_le_bytes(entry[40..48].try_into().unwrap());

            if first == 0 || last < first {
                continue;
            }

            // Clamped to the drive's own capacity.
            let sectors = (last - first + 1).min(self.sectors.saturating_sub(first));

            if sectors == 0 {
                kwarn!("partition {index} starts past the end of the disk");
                continue;
            }

            out.push(Partition::new(*self, first, sectors));
        }

        out
    }
}
