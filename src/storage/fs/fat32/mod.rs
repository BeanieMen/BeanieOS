use crate::kinfo;
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

pub struct Fat32<D: BlockDevice> {
    pub device: Arc<Mutex<D>>,
    pub partition_start: u64,

    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub sectors_per_fat: u32,
    pub root_cluster: u32,
    pub root: Arc<Mutex<Dentry>>,
}

impl<D: BlockDevice> Fat32<D> {
    pub fn mount(device: D, partition_start: u64) -> Result<Arc<Self>, String> {
        let device = Arc::new(Mutex::new(device));
        let mut sector = [0u8; 512];
        device.lock().read_block(partition_start, &mut sector)?;

        kinfo!("FAT32 boot sector: {:x?}", &sector[..]);

        let bytes_per_sector = u16::from_le_bytes([sector[11], sector[12]]);
        if bytes_per_sector == 0 {
            return Err(format!("invalid bytes per sector: {}", bytes_per_sector));
        }

        if bytes_per_sector != 512 {
            return Err(format!("unsupported sector size: {}", bytes_per_sector));
        }

        let sectors_per_cluster = sector[13];

        if sectors_per_cluster == 0 {
            return Err("invalid sectors per cluster".to_string());
        }

        let reserved_sectors = u16::from_le_bytes([sector[14], sector[15]]);

        let fat_count = sector[16];

        if fat_count == 0 {
            return Err("invalid FAT count".to_string());
        }

        let sectors_per_fat = u32::from_le_bytes([sector[36], sector[37], sector[38], sector[39]]);

        if sectors_per_fat == 0 {
            return Err("invalid sectors per FAT".to_string());
        }

        let root_cluster = u32::from_le_bytes([sector[44], sector[45], sector[46], sector[47]]);

        if root_cluster < 2 {
            return Err("invalid root cluster".to_string());
        }

        let inode_ops = Box::leak(Box::new(Fat32InodeOps {
            device: device.clone(),
            partition_start,
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            fat_count,
            sectors_per_fat,
        }));

        let file_ops = Box::leak(Box::new(Fat32FileOps {
            device: device.clone(),
            partition_start,
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            fat_count,
            sectors_per_fat,
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
            partition_start,
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            fat_count,
            sectors_per_fat,
            root_cluster,
            root,
        }))
    }
}

struct Fat32InodeOps<D: BlockDevice> {
    device: Arc<Mutex<D>>,
    partition_start: u64,
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    fat_count: u8,
    sectors_per_fat: u32,
}

struct Fat32FileOps<D: BlockDevice> {
    device: Arc<Mutex<D>>,
    partition_start: u64,
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    fat_count: u8,
    sectors_per_fat: u32,
}

impl<D: BlockDevice> Fat32InodeOps<D> {
    fn first_data_sector(&self) -> u64 {
        self.partition_start
            + self.reserved_sectors as u64
            + self.fat_count as u64 * self.sectors_per_fat as u64
    }
    fn cluster_sector(&self, cluster: u32) -> u64 {
        self.first_data_sector() + (cluster as u64 - 2) * self.sectors_per_cluster as u64
    }

    fn next_cluster(&self, cluster: u32) -> Result<Option<u32>, &'static str> {
        let fat_offset = cluster as u64 * 4;

        let fat_sector = self.partition_start
            + self.reserved_sectors as u64
            + fat_offset / self.bytes_per_sector as u64;

        let entry_offset = (fat_offset % self.bytes_per_sector as u64) as usize;

        let mut sector = [0u8; 512];

        self.device.lock().read_block(fat_sector, &mut sector)?;

        if entry_offset + 4 > sector.len() {
            return Err("invalid FAT entry");
        }

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

impl<D: BlockDevice> InodeOperations for Fat32InodeOps<D> {
    fn lookup(&self, inode: &Inode, name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        if inode.file_type != FileType::Directory {
            return Err("not a directory");
        }

        if name.is_empty() {
            return Err("empty name");
        }

        let mut cluster = inode.cluster;

        loop {
            let first_sector = self.cluster_sector(cluster);

            for sector_index in 0..self.sectors_per_cluster {
                let mut sector = [0u8; 512];

                self.device
                    .lock()
                    .read_block(first_sector + sector_index as u64, &mut sector)?;

                for offset in (0..512).step_by(32) {
                    let entry = &sector[offset..offset + 32];

                    if entry[0] == 0x00 {
                        return Err("file not found");
                    }

                    if entry[0] == 0xE5 {
                        continue;
                    }

                    if entry[11] == 0x0F {
                        continue;
                    }

                    if entry[11] & 0x08 != 0 {
                        continue;
                    }

                    if !Self::short_name_matches(entry, name) {
                        continue;
                    }

                    let high = u16::from_le_bytes([entry[20], entry[21]]) as u32;

                    let low = u16::from_le_bytes([entry[26], entry[27]]) as u32;

                    let child_cluster = (high << 16) | low;

                    let file_type = if entry[11] & 0x10 != 0 {
                        FileType::Directory
                    } else {
                        FileType::Regular
                    };

                    let size =
                        u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]) as u64;

                    let child_inode_ops = Box::leak(Box::new(Fat32InodeOps {
                        device: self.device.clone(),
                        partition_start: self.partition_start,
                        bytes_per_sector: self.bytes_per_sector,
                        sectors_per_cluster: self.sectors_per_cluster,
                        reserved_sectors: self.reserved_sectors,
                        fat_count: self.fat_count,
                        sectors_per_fat: self.sectors_per_fat,
                    }));

                    let child_file_ops = Box::leak(Box::new(Fat32FileOps {
                        device: self.device.clone(),
                        partition_start: self.partition_start,
                        bytes_per_sector: self.bytes_per_sector,
                        sectors_per_cluster: self.sectors_per_cluster,
                        reserved_sectors: self.reserved_sectors,
                        fat_count: self.fat_count,
                        sectors_per_fat: self.sectors_per_fat,
                    }));

                    return Ok(Arc::new(Mutex::new(Inode {
                        id: child_cluster as u64,
                        cluster: child_cluster,
                        file_type,
                        size,
                        inode_ops: child_inode_ops,
                        file_ops: child_file_ops,
                        dentry: None,
                    })));
                }
            }

            match self.next_cluster(cluster)? {
                Some(next) => {
                    cluster = next;
                }
                None => {
                    return Err("file not found");
                }
            }
        }
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
    fn first_data_sector(&self) -> u64 {
        self.partition_start
            + self.reserved_sectors as u64
            + self.fat_count as u64 * self.sectors_per_fat as u64
    }

    fn cluster_sector(&self, cluster: u32) -> u64 {
        self.first_data_sector() + (cluster as u64 - 2) * self.sectors_per_cluster as u64
    }

    fn next_cluster(&self, cluster: u32) -> Result<Option<u32>, &'static str> {
        let fat_offset = cluster as u64 * 4;

        let fat_sector = self.partition_start
            + self.reserved_sectors as u64
            + fat_offset / self.bytes_per_sector as u64;

        let entry_offset = (fat_offset % self.bytes_per_sector as u64) as usize;

        let mut sector = [0u8; 512];

        self.device.lock().read_block(fat_sector, &mut sector)?;

        if entry_offset + 4 > sector.len() {
            return Err("invalid FAT entry");
        }

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

    fn cluster_for_offset(&self, start_cluster: u32, offset: u64) -> Result<u32, &'static str> {
        let cluster_size = self.bytes_per_sector as u64 * self.sectors_per_cluster as u64;

        let mut cluster = start_cluster;

        let count = offset / cluster_size;

        for _ in 0..count {
            cluster = match self.next_cluster(cluster)? {
                Some(next) => next,
                None => return Err("offset past EOF"),
            };
        }

        Ok(cluster)
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

        let available = inode.size - offset;

        let requested = core::cmp::min(buf.len() as u64, available) as usize;

        let cluster_size = self.bytes_per_sector as usize * self.sectors_per_cluster as usize;

        let mut cluster = self.cluster_for_offset(inode.cluster, offset)?;

        let mut position = offset as usize;

        let mut copied = 0usize;

        while copied < requested {
            let cluster_offset = position % cluster_size;

            let remaining_cluster = cluster_size - cluster_offset;

            let count = core::cmp::min(remaining_cluster, requested - copied);

            let first_sector = self.cluster_sector(cluster)
                + (cluster_offset / self.bytes_per_sector as usize) as u64;

            let sector_offset = cluster_offset % self.bytes_per_sector as usize;

            let mut done = 0usize;

            while done < count {
                let sector_number =
                    first_sector + ((sector_offset + done) / self.bytes_per_sector as usize) as u64;

                let offset_in_sector = (sector_offset + done) % self.bytes_per_sector as usize;

                let mut sector = [0u8; 512];

                self.device.lock().read_block(sector_number, &mut sector)?;

                let amount = core::cmp::min(
                    self.bytes_per_sector as usize - offset_in_sector,
                    count - done,
                );

                buf[copied + done..copied + done + amount]
                    .copy_from_slice(&sector[offset_in_sector..offset_in_sector + amount]);

                done += amount;
            }

            copied += count;
            position += count;

            if copied < requested && position % cluster_size == 0 {
                cluster = match self.next_cluster(cluster)? {
                    Some(next) => next,
                    None => {
                        return Err("unexpected end of FAT chain");
                    }
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
