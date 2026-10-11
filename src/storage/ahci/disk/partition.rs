use super::disk::Disk;

// A slice of a disk: one partition table entry.
#[derive(Clone, Copy)]
pub(crate) struct Partition {
    disk: Disk,
    first: u64,
    sectors: u64,
}

impl Partition {
    pub(crate) fn new(disk: Disk, first: u64, sectors: u64) -> Self {
        Partition {
            disk,
            first,
            sectors,
        }
    }

    pub(crate) fn disk(&self) -> &Disk {
        &self.disk
    }

    pub(crate) fn first_lba(&self) -> u64 {
        self.first
    }

    pub(crate) fn sectors(&self) -> u64 {
        self.sectors
    }

    pub(crate) fn byte_offset(&self) -> u64 {
        self.first * self.disk.sector_size()
    }

    pub(crate) fn byte_len(&self) -> u64 {
        self.sectors * self.disk.sector_size()
    }

    pub(crate) fn sector_size(&self) -> u64 {
        self.disk.sector_size()
    }

    pub(crate) fn contains(&self, offset: u64, len: u64) -> bool {
        offset
            .checked_add(len)
            .is_some_and(|end| end <= self.byte_len())
    }

    pub(crate) fn read_at(&mut self, offset: u64, buffer: &mut [u8]) -> Result<(), &'static str> {
        if !self.contains(offset, buffer.len() as u64) {
            return Err("read is past the end of the partition");
        }

        let at = self.byte_offset() + offset;

        self.disk.read_at(at, buffer)
    }

    pub(crate) fn write_at(&mut self, offset: u64, buffer: &[u8]) -> Result<(), &'static str> {
        if !self.contains(offset, buffer.len() as u64) {
            return Err("write is past the end of the partition");
        }

        let at = self.byte_offset() + offset;

        self.disk.write_at(at, buffer)
    }
}
