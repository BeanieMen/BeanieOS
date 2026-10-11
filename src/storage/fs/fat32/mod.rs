use crate::storage::vfs::inode::{FileOperations, Inode, InodeOperations};
use crate::storage::vfs::mount::FileSystem;
use alloc::format;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::{boxed::Box, string::String};
use spin::Mutex;

use crate::{
    kerror,
    storage::{
        ahci::disk::block::BlockDevice,
        vfs::{dentry::Dentry, inode::FileType},
    },
};

// Bytes-per-sector is a runtime value from the BPB, so no buffer may be `[u8; 512]`
// and no `512` may appear in the index arithmetic: on a 4K drive that stepped a
// third of the way through each sector and stitched unrelated entries together.
const MAX_SECTOR: usize = 4096;

// `out` is a maximum-sized scratch buffer, so truncation is what makes the BPB
// value authoritative rather than the buffer.
fn read_sector<D: BlockDevice>(
    device: &Arc<Mutex<D>>,
    lba: u64,
    bytes_per_sector: u16,
    out: &mut [u8],
) -> Result<(), &'static str> {
    let len = bytes_per_sector as usize;

    if out.len() < len {
        return Err("sector is larger than the maximum this kernel reads");
    }

    device.lock().read_block(lba, &mut out[..len])
}

// The BPB fields every cluster-to-sector calculation needs. One definition, so
// the inode path and the file path cannot disagree about where a cluster lives.
#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub sectors_per_fat: u32,
}

impl Geometry {
    fn first_data_sector(&self) -> u64 {
        self.reserved_sectors as u64 + self.fat_count as u64 * self.sectors_per_fat as u64
    }

    fn cluster_sector(&self, cluster: u32) -> u64 {
        self.first_data_sector() + (cluster as u64 - 2) * self.sectors_per_cluster as u64
    }

    fn next_cluster<D: BlockDevice>(
        &self,
        device: &Arc<Mutex<D>>,
        cluster: u32,
    ) -> Result<Option<u32>, &'static str> {
        let fat_offset = cluster as u64 * 4;

        let fat_sector = self.reserved_sectors as u64 + fat_offset / self.bytes_per_sector as u64;

        let entry_offset = (fat_offset % self.bytes_per_sector as u64) as usize;

        if entry_offset + 4 > self.bytes_per_sector as usize {
            return Err("invalid FAT entry");
        }

        let mut sector = [0u8; MAX_SECTOR];

        read_sector(device, fat_sector, self.bytes_per_sector, &mut sector)?;

        let value = u32::from_le_bytes([
            sector[entry_offset],
            sector[entry_offset + 1],
            sector[entry_offset + 2],
            sector[entry_offset + 3],
        ]) & 0x0FFF_FFFF;

        if value >= 0x0FFF_FFF8 {
            Ok(None)
        } else if value == 0x0FFF_FFF7 {
            Err("bad FAT cluster")
        } else if value < 2 {
            Err("invalid FAT cluster")
        } else {
            Ok(Some(value))
        }
    }
}

pub(crate) struct Fat32<D: BlockDevice> {
    pub device: Arc<Mutex<D>>,

    pub geom: Geometry,
    pub root_cluster: u32,
    pub root: Arc<Mutex<Dentry>>,
}

impl<D: BlockDevice> Fat32<D> {
    // Split out of `mount` so the BPB validation is readable on its own.
    fn read_geometry(device: &Arc<Mutex<D>>) -> Result<(Geometry, u32), String> {
        // Full buffer: bytes-per-sector is what this read discovers, so it
        // cannot be the length of the read.
        let mut boot = [0u8; MAX_SECTOR];
        let raw = device.lock().sector_size();

        if raw > MAX_SECTOR {
            return Err(format!(
                "device reports {raw} byte sectors, more than this kernel reads"
            ));
        }

        read_sector(device, 0, raw as u16, &mut boot)?;

        let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]);

        if bytes_per_sector == 0 {
            return Err(format!("invalid bytes per sector: {}", bytes_per_sector));
        }

        // The two have to agree or every offset below is wrong by the ratio
        // between them. Asking the device here is also the only thing that ever
        // called `BlockDevice::sector_size`.
        if bytes_per_sector as usize != raw {
            return Err(format!(
                "BPB says {bytes_per_sector} bytes per sector, device says {raw}"
            ));
        }

        let sectors_per_cluster = boot[13];

        if sectors_per_cluster == 0 {
            return Err("invalid sectors per cluster".to_string());
        }

        let reserved_sectors = u16::from_le_bytes([boot[14], boot[15]]);
        let fat_count = boot[16];

        if fat_count == 0 {
            return Err("invalid FAT count".to_string());
        }

        let sectors_per_fat = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]);

        if sectors_per_fat == 0 {
            return Err("invalid sectors per FAT".to_string());
        }

        let root_cluster = u32::from_le_bytes([boot[44], boot[45], boot[46], boot[47]]);

        if root_cluster < 2 {
            return Err("invalid root cluster".to_string());
        }

        Ok((
            Geometry {
                bytes_per_sector,
                sectors_per_cluster,
                reserved_sectors,
                fat_count,
                sectors_per_fat,
            },
            root_cluster,
        ))
    }

    // `device` is a `Partition`, not a `Disk`: FAT32 numbers sectors from its own
    // volume boot record, and `Partition::read_at` turns that back into a disk
    // offset and refuses a read past the end.
    pub(crate) fn mount(device: D) -> Result<Arc<Self>, String> {
        let device = Arc::new(Mutex::new(device));
        let (geom, root_cluster) = Self::read_geometry(&device)?;

        let inode_ops = Box::leak(Box::new(Fat32InodeOps {
            device: device.clone(),
            geom,
        }));

        let file_ops = Box::leak(Box::new(Fat32FileOps {
            device: device.clone(),
            geom,
        }));

        let inode = Arc::new(Mutex::new(Inode {
            id: root_cluster as u64,
            cluster: root_cluster,
            file_type: FileType::Directory,
            size: 0,
            inode_ops,
            file_ops,
            dentry: None,
        }));

        let root = Arc::new(Mutex::new(Dentry {
            name: String::from("/"),
            inode: inode.clone(),
            parent: None,
        }));

        inode.lock().dentry = Some(root.clone());

        Ok(Arc::new(Self {
            device,
            geom,
            root_cluster,
            root,
        }))
    }
}

struct Fat32InodeOps<D: BlockDevice> {
    device: Arc<Mutex<D>>,
    geom: Geometry,
}

struct Fat32FileOps<D: BlockDevice> {
    device: Arc<Mutex<D>>,
    geom: Geometry,
}

impl<D: BlockDevice> Fat32InodeOps<D> {
    fn short_name_matches(entry: &[u8], name: &[u8]) -> bool {
        if entry.len() < 11 {
            return false;
        }

        let mut target = [b' '; 11];

        let mut parts = name.splitn(2, |b| *b == b'.');

        let base = parts.next().unwrap_or(&[]);

        let ext = parts.next().unwrap_or(&[]);

        if base.is_empty() || base.len() > 8 || ext.len() > 3 {
            return false;
        }

        for (i, byte) in base.iter().enumerate() {
            target[i] = byte.to_ascii_uppercase();
        }

        for (i, byte) in ext.iter().enumerate() {
            target[8 + i] = byte.to_ascii_uppercase();
        }

        entry[..11] == target
    }
}

impl<D: BlockDevice> Fat32InodeOps<D> {
    // Walks `start_cluster`'s chain for `name`. Returns the raw 32-byte entry, not a
    // built inode, so the chain walk and the inode construction stay separate.
    fn find_entry(&self, start_cluster: u32, name: &[u8]) -> Result<[u8; 32], &'static str> {
        let mut cluster = start_cluster;

        loop {
            let first_sector = self.geom.cluster_sector(cluster);

            for sector_index in 0..self.geom.sectors_per_cluster {
                let mut sector = [0u8; MAX_SECTOR];

                read_sector(
                    &self.device,
                    first_sector + sector_index as u64,
                    self.geom.bytes_per_sector,
                    &mut sector,
                )?;

                for offset in (0..self.geom.bytes_per_sector as usize).step_by(32) {
                    let mut entry = [0u8; 32];
                    entry.copy_from_slice(&sector[offset..offset + 32]);

                    // 0x00 ends the directory, 0xE5 is a deleted entry, 0x0F is
                    // a long-filename fragment, 0x08 is the volume label.
                    if entry[0] == 0x00 {
                        return Err("file not found");
                    }

                    if entry[0] == 0xE5 || entry[11] == 0x0F || entry[11] & 0x08 != 0 {
                        continue;
                    }

                    if Self::short_name_matches(&entry, name) {
                        return Ok(entry);
                    }
                }
            }

            match self.geom.next_cluster(&self.device, cluster)? {
                Some(next) => cluster = next,
                None => return Err("file not found"),
            }
        }
    }

    fn inode_from_entry(&self, entry: &[u8; 32]) -> Arc<Mutex<Inode>> {
        let high = u16::from_le_bytes([entry[20], entry[21]]) as u32;
        let low = u16::from_le_bytes([entry[26], entry[27]]) as u32;

        let cluster = (high << 16) | low;

        let file_type = if entry[11] & 0x10 != 0 {
            FileType::Directory
        } else {
            FileType::Regular
        };

        let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]) as u64;

        let inode_ops = Box::leak(Box::new(Fat32InodeOps {
            device: self.device.clone(),
            geom: self.geom,
        }));

        let file_ops = Box::leak(Box::new(Fat32FileOps {
            device: self.device.clone(),
            geom: self.geom,
        }));

        Arc::new(Mutex::new(Inode {
            id: cluster as u64,
            cluster,
            file_type,
            size,
            inode_ops,
            file_ops,
            dentry: None,
        }))
    }
}

impl<D: BlockDevice> InodeOperations for Fat32InodeOps<D> {
    fn lookup(&self, inode: &Inode, name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        if inode.file_type != FileType::Directory {
            return Err("not a directory");
        }

        if name.is_empty() {
            return Err("empty name");
        }

        let entry = self.find_entry(inode.cluster, name)?;

        Ok(self.inode_from_entry(&entry))
    }
    fn create(
        &self,
        _inode: &mut Inode,
        _name: &[u8],
        _file_type: FileType,
    ) -> Result<Arc<Mutex<Inode>>, &'static str> {
        kerror!("FAT32: create not implemented");
        Err("not implemented")
    }

    fn mkdir(&self, _inode: &mut Inode, _name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        kerror!("FAT32: mkdir not implemented");
        Err("not implemented")
    }

    fn rmdir(&self, _inode: &mut Inode, _name: &[u8]) -> Result<(), &'static str> {
        kerror!("FAT32: rmdir not implemented");
        Err("not implemented")
    }

    fn unlink(&self, _inode: &mut Inode, _name: &[u8]) -> Result<(), &'static str> {
        kerror!("FAT32: unlink not implemented");
        Err("not implemented")
    }
}

impl<D: BlockDevice> Fat32FileOps<D> {
    fn cluster_for_offset(&self, start_cluster: u32, offset: u64) -> Result<u32, &'static str> {
        let cluster_size = self.geom.bytes_per_sector as u64 * self.geom.sectors_per_cluster as u64;

        let mut cluster = start_cluster;

        let count = offset / cluster_size;

        for _ in 0..count {
            cluster = match self.geom.next_cluster(&self.device, cluster)? {
                Some(next) => next,
                None => return Err("offset past EOF"),
            };
        }

        Ok(cluster)
    }
}

impl<D: BlockDevice> Fat32FileOps<D> {
    // Copies one run of bytes from within a single sector.
    fn read_within_sector(
        &self,
        sector_number: u64,
        offset_in_sector: usize,
        count: usize,
        out: &mut [u8],
    ) -> Result<(), &'static str> {
        let mut sector = [0u8; MAX_SECTOR];

        read_sector(
            &self.device,
            sector_number,
            self.geom.bytes_per_sector,
            &mut sector,
        )?;

        out.copy_from_slice(&sector[offset_in_sector..offset_in_sector + count]);

        Ok(())
    }
}

impl<D: BlockDevice> FileOperations for Fat32FileOps<D> {
    fn read(&self, inode: &Inode, offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        if inode.file_type != FileType::Regular {
            return Err("not a regular file");
        }

        if offset >= inode.size || buf.is_empty() {
            return Ok(0);
        }

        let requested = core::cmp::min(buf.len() as u64, inode.size - offset) as usize;

        let bytes_per_sector = self.geom.bytes_per_sector as usize;
        let cluster_size = bytes_per_sector * self.geom.sectors_per_cluster as usize;

        let mut cluster = self.cluster_for_offset(inode.cluster, offset)?;
        let mut position = offset as usize;
        let mut copied = 0usize;

        while copied < requested {
            let cluster_offset = position % cluster_size;
            let count = core::cmp::min(cluster_size - cluster_offset, requested - copied);

            let first_sector =
                self.geom.cluster_sector(cluster) + (cluster_offset / bytes_per_sector) as u64;

            let sector_offset = cluster_offset % bytes_per_sector;

            let mut done = 0usize;

            while done < count {
                let amount = core::cmp::min(
                    bytes_per_sector - sector_offset % bytes_per_sector,
                    count - done,
                );

                self.read_within_sector(
                    first_sector + ((sector_offset + done) / bytes_per_sector) as u64,
                    (sector_offset + done) % bytes_per_sector,
                    amount,
                    &mut buf[copied + done..copied + done + amount],
                )?;

                done += amount;
            }

            copied += count;
            position += count;

            if copied < requested && position % cluster_size == 0 {
                cluster = match self.geom.next_cluster(&self.device, cluster)? {
                    Some(next) => next,
                    None => return Err("unexpected end of FAT chain"),
                };
            }
        }

        Ok(copied)
    }

    fn write(&self, _inode: &mut Inode, _offset: u64, _buf: &[u8]) -> Result<usize, &'static str> {
        kerror!("FAT32: write not implemented");
        Err("not implemented")
    }
}

impl<D: BlockDevice + 'static> FileSystem for Fat32<D> {
    fn root(&self) -> Arc<Mutex<Dentry>> {
        self.root.clone()
    }

    fn sync(&self) -> Result<(), &'static str> {
        Ok(())
    }
}
