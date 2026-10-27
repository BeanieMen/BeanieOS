use crate::storage::vfs::inode::{FileOperations, Inode, InodeOperations, RawEntry};
use crate::storage::vfs::mount::FileSystem;
use alloc::format;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::{boxed::Box, string::String};
use spin::Mutex;

use crate::storage::{
    ahci::disk::block::BlockDevice,
    vfs::{dentry::Dentry, inode::FileType},
};

const MAX_SECTOR: usize = 4096;

const FAT_ENTRY_MASK: u64 = 0x0FFF_FFFF;
const FIRST_DATA_CLUSTER: u32 = 2;
const CLUSTER_EOC_MIN: u32 = 0x0FFF_FFF8;
const CLUSTER_BAD: u32 = 0x0FFF_FFF7;

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

fn write_sector<D: BlockDevice>(
    device: &Arc<Mutex<D>>,
    lba: u64,
    bytes_per_sector: u16,
    data: &[u8],
) -> Result<(), &'static str> {
    let len = bytes_per_sector as usize;

    if data.len() != len {
        return Err("write must cover exactly one sector");
    }

    device.lock().write_block(lba, data)
}

#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub sectors_per_fat: u32,
    /// One past the highest valid data cluster number, from the BPB total sector
    /// count and the FAT size (never above what the FAT itself can describe).
    pub cluster_limit: u32,
    /// Per-mount advisory start for the free-cluster scan. A wrong value only changes
    /// allocation order: the scan still checks every FAT entry it reads.
    pub alloc_hint: &'static core::sync::atomic::AtomicU32,
}

static FAT_CACHE: Mutex<Option<(usize, u64, [u8; MAX_SECTOR])>> = Mutex::new(None);

impl Geometry {
    // One clean, device-keyed FAT sector. Writes remain synchronous; a failed
    // device write invalidates the entry rather than publishing modified bytes.
    fn read_fat_sector<D: BlockDevice>(
        &self, device: &Arc<Mutex<D>>, lba: u64, out: &mut [u8; MAX_SECTOR],
    ) -> Result<(), &'static str> {
        let len = self.bytes_per_sector as usize;
        let mut cache = FAT_CACHE.lock();
        let key = Arc::as_ptr(device) as usize;
        if let Some((cached_device, cached_sector, cached_data)) = &*cache
            && *cached_device == key && *cached_sector == lba {
            out[..len].copy_from_slice(&cached_data[..len]);
        } else {
            read_sector(device, lba, self.bytes_per_sector, out)?;
            *cache = Some((key, lba, *out));
        }
        Ok(())
    }

    fn first_data_sector(&self) -> u64 {
        self.reserved_sectors as u64 + self.fat_count as u64 * self.sectors_per_fat as u64
    }

    fn cluster_sector(&self, cluster: u32) -> Result<u64, &'static str> {
        if cluster<FIRST_DATA_CLUSTER || cluster>=self.cluster_limit{return Err("cluster outside volume");}
        let offset=(cluster as u64).checked_sub(FIRST_DATA_CLUSTER as u64)
            .and_then(|n|n.checked_mul(self.sectors_per_cluster as u64)).ok_or("cluster offset overflow")?;
        self.first_data_sector().checked_add(offset).ok_or("cluster sector overflow")
    }

    fn next_cluster<D: BlockDevice>(
        &self,
        device: &Arc<Mutex<D>>,
        cluster: u32,
    ) -> Result<Option<u32>, &'static str> {
        if cluster<FIRST_DATA_CLUSTER || cluster>=self.cluster_limit{return Err("cluster outside volume");}
        let fat_offset = cluster as u64 * 4;

        let fat_sector = self.reserved_sectors as u64 + fat_offset / self.bytes_per_sector as u64;

        let entry_offset = (fat_offset % self.bytes_per_sector as u64) as usize;

        if entry_offset + 4 > self.bytes_per_sector as usize {
            return Err("invalid FAT entry");
        }

        // Read one entry directly from the clean cached sector. On a miss,
        // publish only a successful full-sector read, as read_fat_sector does.
        let value = {
            let mut cache = FAT_CACHE.lock();
            let key = Arc::as_ptr(device) as usize;
            let hit = matches!(&*cache, Some((dev, lba, _)) if *dev == key && *lba == fat_sector);
            if !hit {
                let mut sector = [0u8; MAX_SECTOR];
                read_sector(device, fat_sector, self.bytes_per_sector, &mut sector)?;
                *cache = Some((key, fat_sector, sector));
            }
            let Some((_, _, sector)) = &*cache else { unreachable!() };
            u32::from_le_bytes(sector[entry_offset..entry_offset + 4].try_into().unwrap())
                & FAT_ENTRY_MASK as u32
        };

        if value >= CLUSTER_EOC_MIN {
            Ok(None)
        } else if value == CLUSTER_BAD {
            Err("bad FAT cluster")
        } else if value < FIRST_DATA_CLUSTER {
            Err("invalid FAT cluster")
        } else if value>=self.cluster_limit {
            Err("FAT successor outside volume")
        } else {
            Ok(Some(value))
        }
    }

    fn set_fat_entry<D: BlockDevice>(
        &self,
        device: &Arc<Mutex<D>>,
        cluster: u32,
        value: u32,
    ) -> Result<(), &'static str> {
        *CLUSTER_OFFSET_CACHE.lock() = [None; 16];

        if cluster<FIRST_DATA_CLUSTER || cluster>=self.cluster_limit{return Err("cluster outside volume");}
        let fat_offset = cluster as u64 * 4;

        let fat_sector = self.reserved_sectors as u64 + fat_offset / self.bytes_per_sector as u64;

        let entry_offset = (fat_offset % self.bytes_per_sector as u64) as usize;

        if entry_offset + 4 > self.bytes_per_sector as usize {
            return Err("invalid FAT entry");
        }

        let mut sector = [0u8; MAX_SECTOR];

        self.read_fat_sector(device, fat_sector, &mut sector)?;

        let bytes = (value & FAT_ENTRY_MASK as u32).to_le_bytes();

        sector[entry_offset..entry_offset + 4].copy_from_slice(&bytes);

        let len = self.bytes_per_sector as usize;

        let result = write_sector(device, fat_sector, self.bytes_per_sector, &sector[..len]);
        let mut cache = FAT_CACHE.lock();
        *cache = if result.is_ok() {
            Some((Arc::as_ptr(device) as usize, fat_sector, sector))
        } else {
            None
        };
        result
    }

    fn allocate_cluster<D: BlockDevice>(
        &self,
        device: &Arc<Mutex<D>>,
    ) -> Result<u32, &'static str> {
        let mut sector = [0u8; MAX_SECTOR];

        let limit = self.cluster_limit as u64;
        // FAT sectors that can hold entries below the limit.
        let fat_sectors = ((limit * 4).div_ceil(self.bytes_per_sector as u64))
            .min(self.sectors_per_fat as u64);
        let hint = self.alloc_hint.load(core::sync::atomic::Ordering::Relaxed) as u64;
        let start = if hint >= FIRST_DATA_CLUSTER as u64 && hint < limit {
            (hint * 4) / self.bytes_per_sector as u64
        } else {
            0
        };

        for index in (start..fat_sectors).chain(0..start) {
            self.read_fat_sector(device, self.reserved_sectors as u64 + index, &mut sector)?;

            let len = self.bytes_per_sector as usize;

            for offset in (0..len).step_by(4) {
                let value = u32::from_le_bytes([
                    sector[offset],
                    sector[offset + 1],
                    sector[offset + 2],
                    sector[offset + 3],
                ]);

                if value & FAT_ENTRY_MASK as u32 != 0 {
                    continue;
                }

                let cluster64 = index * len as u64 / 4 + offset as u64 / 4;

                if cluster64 < FIRST_DATA_CLUSTER as u64 {
                    continue;
                }

                if cluster64 >= limit {
                    break;
                }

                let cluster = cluster64 as u32;

                self.set_fat_entry(device, cluster, 0x0FFF_FFFF)?;

                self.alloc_hint
                    .store(cluster + 1, core::sync::atomic::Ordering::Relaxed);

                return Ok(cluster);
            }
        }

        Err("no free cluster")
    }

    fn free_cluster<D: BlockDevice>(
        &self,
        device: &Arc<Mutex<D>>,
        cluster: u32,
    ) -> Result<(), &'static str> {
        if cluster >= FIRST_DATA_CLUSTER && cluster < self.cluster_limit {
            self.alloc_hint
                .fetch_min(cluster, core::sync::atomic::Ordering::Relaxed);
        }
        self.set_fat_entry(device, cluster, 0)
    }
}

pub const FAT_INODE_BASE: u64 = 0x1_0000_0000;

pub(crate) struct Fat32<D: BlockDevice> {
    pub device: Arc<Mutex<D>>,

    pub geom: Geometry,
    pub root_cluster: u32,
    pub root: Arc<Mutex<Dentry>>,
    inode_ops: &'static Fat32InodeOps<D>,
    file_ops: &'static Fat32FileOps<D>,
}

impl<D: BlockDevice> Fat32<D> {
    fn read_geometry(device: &Arc<Mutex<D>>) -> Result<(Geometry, u32), String> {
        let mut boot = [0u8; MAX_SECTOR];
        let raw = device.lock().sector_size();

        if raw > MAX_SECTOR {
            return Err(format!(
                "device reports {raw} byte sectors, more than this kernel reads"
            ));
        }

        read_sector(device, 0, raw as u16, &mut boot)?;

        let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]);

        if !matches!(bytes_per_sector,512|1024|2048|4096) {
            return Err(format!("invalid bytes per sector: {}", bytes_per_sector));
        }

        if bytes_per_sector as usize != raw {
            return Err(format!(
                "BPB says {bytes_per_sector} bytes per sector, device says {raw}"
            ));
        }

        let sectors_per_cluster = boot[13];

        if !sectors_per_cluster.is_power_of_two() || sectors_per_cluster>128 {
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

        let total16 = u16::from_le_bytes([boot[19], boot[20]]) as u64;
        let total32 = u32::from_le_bytes([boot[32], boot[33], boot[34], boot[35]]) as u64;
        let total_sectors = (if total32 != 0 { total32 } else { total16 }).min(device.lock().sector_count());

        let first_data = reserved_sectors as u64 + fat_count as u64 * sectors_per_fat as u64;

        let Some(data_sectors) = total_sectors.checked_sub(first_data) else {
            return Err("BPB total sectors smaller than the FAT area".to_string());
        };

        let data_clusters = data_sectors / sectors_per_cluster as u64;
        // Highest valid FAT32 cluster is 0x0FFFFFF6; the FAT must also be able to hold it.
        let fat_capacity = sectors_per_fat as u64 * bytes_per_sector as u64 / 4;
        let cluster_limit = (2 + data_clusters)
            .min(fat_capacity)
            .min(0x0FFF_FFF7) as u32;

        if cluster_limit <= root_cluster {
            return Err("root cluster outside the volume".to_string());
        }

        Ok((
            Geometry {
                bytes_per_sector,
                sectors_per_cluster,
                reserved_sectors,
                fat_count,
                sectors_per_fat,
                cluster_limit,
                alloc_hint: Box::leak(Box::new(core::sync::atomic::AtomicU32::new(u32::MAX))),
            },
            root_cluster,
        ))
    }

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
            id: FAT_INODE_BASE | (root_cluster as u64),
            cluster: root_cluster,
            file_type: FileType::Directory,
            size: 0,
            inode_ops,
            file_ops,
            dentry: None,
            parent_cluster: 0,
            dir_name: *b"/          ",
        }));

        let root = Arc::new(Mutex::new(Dentry {
            name: String::from("/"),
            inode: inode.clone(),
            parent: None,
        }));

        inode.lock().dentry = Some(Arc::downgrade(&root));

        Ok(Arc::new(Self {
            device,
            geom,
            root_cluster,
            root,
            inode_ops,
            file_ops,
        }))
    }
}

fn lfn_checksum(short_name: &[u8]) -> u8 {
    let mut sum: u8 = 0;
    for &b in &short_name[..11] {
        sum = (sum.rotate_right(1)).wrapping_add(b);
    }
    sum
}

fn format_8_3(entry: &[u8; 32]) -> ([u8; 256], usize) {
    let mut out = [0u8; 256];
    let mut len = 0;
    if entry[0] == b'.' {
        out[0] = b'.';
        if entry[1] == b'.' {
            out[1] = b'.';
            return (out, 2);
        }
        return (out, 1);
    }
    let base = &entry[0..8];
    let ext = &entry[8..11];
    let base_len = base.iter().rposition(|&b| b != b' ').map_or(0, |p| p + 1);
    let ext_len = ext.iter().rposition(|&b| b != b' ').map_or(0, |p| p + 1);
    let lower_base = entry[12] & 0x08 != 0;
    let lower_ext = entry[12] & 0x10 != 0;
    for &b in &base[..base_len] {
        out[len] = if lower_base { b.to_ascii_lowercase() } else { b };
        len += 1;
    }
    if ext_len > 0 {
        out[len] = b'.';
        len += 1;
        for &b in &ext[..ext_len] {
            out[len] = if lower_ext { b.to_ascii_lowercase() } else { b };
            len += 1;
        }
    }
    (out, len)
}

struct LfnCollector {
    chars: [u16; 260],
    checksum: u8,
    valid: bool,
}

impl LfnCollector {
    fn new() -> Self {
        Self {
            chars: [0u16; 260],
            checksum: 0,
            valid: false,
        }
    }

    fn reset(&mut self) {
        self.chars = [0u16; 260];
        self.checksum = 0;
        self.valid = false;
    }

    fn process_lfn_entry(&mut self, entry: &[u8; 32]) {
        let seq = entry[0] & 0x1F;
        let is_last = (entry[0] & 0x40) != 0;

        if is_last {
            self.chars = [0u16; 260];
            self.checksum = entry[13];
            self.valid = true;
        } else if !self.valid || self.checksum != entry[13] {
            self.valid = false;
            return;
        }

        if seq >= 1 && seq <= 20 {
            let base = (seq as usize - 1) * 13;
            let pairs = [
                (1, 2), (3, 4), (5, 6), (7, 8), (9, 10),
                (14, 15), (16, 17), (18, 19), (20, 21), (22, 23), (24, 25),
                (28, 29), (30, 31),
            ];
            for (i, &(l, h)) in pairs.iter().enumerate() {
                if base + i < 260 {
                    self.chars[base + i] = u16::from_le_bytes([entry[l], entry[h]]);
                }
            }
        }
    }

    fn get_name(&self, short_entry: &[u8; 32]) -> Option<([u8; 256], usize)> {
        if !self.valid || self.checksum != lfn_checksum(&short_entry[..11]) {
            return None;
        }
        let mut out = [0u8; 256];
        let mut len = 0;
        for &c in &self.chars {
            if c == 0 || c == 0xFFFF {
                break;
            }
            if c < 0x80 {
                if len < 256 {
                    out[len] = c as u8;
                    len += 1;
                }
            } else if c < 0x800 {
                if len + 1 < 256 {
                    out[len] = (0xC0 | (c >> 6)) as u8;
                    out[len + 1] = (0x80 | (c & 0x3F)) as u8;
                    len += 2;
                }
            } else {
                if len + 2 < 256 {
                    out[len] = (0xE0 | (c >> 12)) as u8;
                    out[len + 1] = (0x80 | ((c >> 6) & 0x3F)) as u8;
                    out[len + 2] = (0x80 | (c & 0x3F)) as u8;
                    len += 3;
                }
            }
        }
        if len > 0 {
            Some((out, len))
        } else {
            None
        }
    }

    fn matches(&self, short_entry: &[u8; 32], target: &[u8]) -> bool {
        if let Some((name, len)) = self.get_name(short_entry) {
            let name_slice = &name[..len];
            if name_slice.len() == target.len() {
                return name_slice
                    .iter()
                    .zip(target.iter())
                    .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase());
            }
        }
        false
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

        if name == b"." {
            return entry[0] == b'.' && entry[1] == b' ';
        }

        if name == b".." {
            return entry[0] == b'.' && entry[1] == b'.';
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
    fn free_chain(&self, cluster: u32) -> Result<(), &'static str> {
        let mut next = Some(cluster);
        let mut hops = 0u64;
        let max_hops = self.geom.sectors_per_fat as u64 * 128;
        while let Some(c) = next {
            let successor = self.geom.next_cluster(&self.device, c)?;
            self.geom.free_cluster(&self.device, c)?;
            hops += 1;
            if hops >= max_hops {
                return Err("corrupt FAT chain");
            }
            next = successor;
        }
        Ok(())
    }

    fn find_entry(&self, start_cluster: u32, name: &[u8]) -> Result<[u8; 32], &'static str> {
        let mut cluster = start_cluster;
        let mut hops = 0u64;
        let max_hops = self.geom.sectors_per_fat as u64 * 128;
        let mut lfn = LfnCollector::new();

        loop {
            let first_sector = self.geom.cluster_sector(cluster)?;

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

                    if entry[0] == 0x00 {
                        return Err("file not found");
                    }

                    if entry[0] == 0xE5 {
                        lfn.reset();
                        continue;
                    }

                    if entry[11] == 0x0F {
                        lfn.process_lfn_entry(&entry);
                        continue;
                    }

                    if entry[11] & 0x08 != 0 {
                        lfn.reset();
                        continue;
                    }

                    if lfn.matches(&entry, name) || Self::short_name_matches(&entry, name) {
                        return Ok(entry);
                    }

                    lfn.reset();
                }
            }

            hops += 1;
            if hops >= max_hops {
                return Err("corrupt FAT chain");
            }
            match self.geom.next_cluster(&self.device, cluster)? {
                Some(next) => cluster = next,
                None => return Err("file not found"),
            }
        }
    }

    fn entry_at(&self, start_cluster: u32, index: u64) -> Result<Option<RawEntry>, &'static str> {
        let mut cluster = start_cluster;
        let mut valid_count = 0u64;
        let mut raw_slot = 0u64;
        let mut hops = 0u64;
        let max_hops = self.geom.sectors_per_fat as u64 * 128;
        let mut lfn = LfnCollector::new();

        loop {
            let first_sector = self.geom.cluster_sector(cluster)?;

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

                    if entry[0] == 0x00 {
                        return Ok(None);
                    }

                    let slot = raw_slot;
                    raw_slot += 1;

                    if entry[0] == 0xE5 {
                        lfn.reset();
                        continue;
                    }

                    if entry[11] == 0x0F {
                        lfn.process_lfn_entry(&entry);
                        continue;
                    }

                    if entry[11] & 0x08 != 0 {
                        lfn.reset();
                        continue;
                    }

                    if valid_count == index {
                        let (name, name_len) = if let Some((lfn_name, len)) = lfn.get_name(&entry) {
                            (lfn_name, len)
                        } else {
                            format_8_3(&entry)
                        };

                        let high = u16::from_le_bytes([entry[20], entry[21]]) as u32;
                        let low = u16::from_le_bytes([entry[26], entry[27]]) as u32;

                        let file_type = if entry[11] & 0x10 != 0 {
                            FileType::Directory
                        } else {
                            FileType::Regular
                        };

                        return Ok(Some(RawEntry {
                            inode: (((high << 16) | low) as u64) | (slot << 32),
                            file_type,
                            size: u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]),
                            name,
                            name_len,
                        }));
                    }

                    valid_count += 1;
                    lfn.reset();
                }
            }

            hops += 1;
            if hops >= max_hops {
                return Err("corrupt FAT chain");
            }
            match self.geom.next_cluster(&self.device, cluster)? {
                Some(next) => cluster = next,
                None => return Ok(None),
            }
        }
    }

    fn free_slot(&self, start_cluster: u32) -> Result<(u64, usize), &'static str> {
        let mut cluster = start_cluster;
        let mut hops = 0u64;
        let max_hops = self.geom.sectors_per_fat as u64 * 128;

        loop {
            let first_sector = self.geom.cluster_sector(cluster)?;

            for sector_index in 0..self.geom.sectors_per_cluster {
                let sector_number = first_sector + sector_index as u64;

                let mut sector = [0u8; MAX_SECTOR];

                read_sector(
                    &self.device,
                    sector_number,
                    self.geom.bytes_per_sector,
                    &mut sector,
                )?;

                let len = self.geom.bytes_per_sector as usize;

                for offset in (0..len).step_by(32) {
                    let first = sector[offset];

                    if first == 0x00 || first == 0xE5 {
                        return Ok((sector_number, offset));
                    }
                }
            }

            hops += 1;
            if hops >= max_hops {
                return Err("corrupt FAT chain");
            }
            match self.geom.next_cluster(&self.device, cluster)? {
                Some(next) => cluster = next,
                None => return Err("directory is full"),
            }
        }
    }

    fn directory_entry(
        name: &[u8],
        cluster: u32,
        file_type: FileType,
    ) -> Result<[u8; 32], &'static str> {
        if name.is_empty() || name.len() > 12 {
            return Err("bad name for FAT32");
        }

        let mut entry = [0u8; 32];
        entry[0..11].fill(b' ');

        let (base, ext) = match name.iter().position(|b| *b == b'.') {
            Some(dot) => (&name[..dot], &name[dot + 1..]),
            None => (name, &name[0..0]),
        };

        if base.is_empty() || base.len() > 8 || ext.len() > 3 {
            return Err("bad name for FAT32");
        }

        for (i, byte) in base.iter().enumerate() {
            entry[i] = byte.to_ascii_uppercase();
        }

        for (i, byte) in ext.iter().enumerate() {
            entry[8 + i] = byte.to_ascii_uppercase();
        }

        entry[11] = if file_type == FileType::Directory {
            0x10
        } else {
            0x20
        };

        entry[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
        entry[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
        entry[28..32].copy_from_slice(&0u32.to_le_bytes());

        Ok(entry)
    }

    /// Builds the `.` and `..` pair a new directory starts with.
    ///
    /// These two names are not valid 8.3 components, so they are laid down as
    /// raw name bytes instead of going through `directory_entry`.
    fn self_entries(
        parent_cluster: u32,
        self_cluster: u32,
        file_type: FileType,
    ) -> Result<([u8; 32], [u8; 32]), &'static str> {
        let attr = if file_type == FileType::Directory {
            0x10u8
        } else {
            0x20u8
        };

        let mut me = [0u8; 32];
        me[0..11].fill(b' ');
        me[0] = b'.';
        me[11] = attr;
        me[20..22].copy_from_slice(&((self_cluster >> 16) as u16).to_le_bytes());
        me[26..28].copy_from_slice(&(self_cluster as u16).to_le_bytes());
        me[28..32].copy_from_slice(&0u32.to_le_bytes());

        let up_cluster = if parent_cluster <= 2 { 0 } else { parent_cluster };
        let mut up = [0u8; 32];
        up[0..11].fill(b' ');
        up[0] = b'.';
        up[1] = b'.';
        up[11] = attr;
        up[20..22].copy_from_slice(&((up_cluster >> 16) as u16).to_le_bytes());
        up[26..28].copy_from_slice(&(up_cluster as u16).to_le_bytes());
        up[28..32].copy_from_slice(&0u32.to_le_bytes());

        Ok((me, up))
    }

    fn is_empty_dir(
        device: &Arc<Mutex<D>>,
        geom: Geometry,
        cluster: u32,
    ) -> Result<bool, &'static str> {
        let mut cluster = cluster;
        let mut hops = 0u64;
        let max_hops = geom.sectors_per_fat as u64 * 128;

        loop {
            let first_sector = geom.cluster_sector(cluster)?;

            for sector_index in 0..geom.sectors_per_cluster {
                let mut sector = [0u8; MAX_SECTOR];

                read_sector(
                    device,
                    first_sector + sector_index as u64,
                    geom.bytes_per_sector,
                    &mut sector,
                )?;

                for offset in (0..geom.bytes_per_sector as usize).step_by(32) {
                    let byte = sector[offset];

                    if byte == 0x00 {
                        return Ok(true);
                    }

                    if byte == 0xE5 || sector[offset + 11] == 0x0F {
                        continue;
                    }

                    if byte == b'.' && (sector[offset + 1] == b' ' || sector[offset + 1] == b'.') {
                        continue;
                    }

                    return Ok(false);
                }
            }

            hops += 1;
            if hops >= max_hops {
                return Err("corrupt FAT chain");
            }
            match geom.next_cluster(device, cluster)? {
                Some(next) => cluster = next,
                None => return Ok(true),
            }
        }
    }

    fn mark_deleted(
        device: &Arc<Mutex<D>>,
        geom: Geometry,
        dir_cluster: u32,
        entry: &mut [u8; 32],
    ) -> Result<(), &'static str> {
        let mut cluster = dir_cluster;
        let mut hops = 0u64;
        let max_hops = geom.sectors_per_fat as u64 * 128;

        loop {
            let first_sector = geom.cluster_sector(cluster)?;

            for sector_index in 0..geom.sectors_per_cluster {
                let sector_number = first_sector + sector_index as u64;

                let mut sector = [0u8; MAX_SECTOR];

                read_sector(device, sector_number, geom.bytes_per_sector, &mut sector)?;

                let len = geom.bytes_per_sector as usize;

                for offset in (0..len).step_by(32) {
                    if sector[offset..offset + 32] == *entry {
                        sector[offset] = 0xE5;
                        write_sector(device, sector_number, geom.bytes_per_sector, &sector[..len])?;
                        entry[0] = 0xE5;

                        return Ok(());
                    }
                }
            }

            hops += 1;
            if hops >= max_hops {
                return Err("corrupt FAT chain");
            }
            match geom.next_cluster(device, cluster)? {
                Some(next) => cluster = next,
                None => return Err("entry vanished"),
            }
        }
    }

    fn inode_from_entry(
        &self,
        entry: &[u8; 32],
        parent_cluster: u32,
        inode_ops: &'static dyn InodeOperations,
        file_ops: &'static dyn FileOperations,
    ) -> Arc<Mutex<Inode>> {
        let high = u16::from_le_bytes([entry[20], entry[21]]) as u32;
        let low = u16::from_le_bytes([entry[26], entry[27]]) as u32;

        let cluster = (high << 16) | low;

        let file_type = if entry[11] & 0x10 != 0 {
            FileType::Directory
        } else {
            FileType::Regular
        };

        let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]) as u64;

        let mut dir_name = [b' '; 11];
        dir_name.copy_from_slice(&entry[..11]);

        Arc::new(Mutex::new(Inode {
            id: FAT_INODE_BASE | (cluster as u64),
            cluster,
            file_type,
            size,
            inode_ops,
            file_ops,
            dentry: None,
            parent_cluster,
            dir_name,
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

        Ok(self.inode_from_entry(&entry, inode.cluster, inode.inode_ops, inode.file_ops))
    }
    fn create(
        &self,
        inode: &mut Inode,
        name: &[u8],
        file_type: FileType,
    ) -> Result<Arc<Mutex<Inode>>, &'static str> {
        if inode.file_type != FileType::Directory {
            return Err("not a directory");
        }

        if self.find_entry(inode.cluster, name).is_ok() {
            return Err("file already exists");
        }

        let parent_cluster = inode.cluster;

        let cluster = self.geom.allocate_cluster(&self.device)?;

        if file_type == FileType::Directory {
            let first_sector = self.geom.cluster_sector(cluster)?;
            let len = self.geom.bytes_per_sector as usize;

            let mut sector = [0u8; MAX_SECTOR];
            sector[len..].fill(0);

            for sector_index in 0..self.geom.sectors_per_cluster {
                write_sector(
                    &self.device,
                    first_sector + sector_index as u64,
                    self.geom.bytes_per_sector,
                    &sector[..len],
                )?;
            }

            let (me, up) = Self::self_entries(parent_cluster, cluster, file_type)?;

            write_sector(
                &self.device,
                first_sector,
                self.geom.bytes_per_sector,
                &sector[..len],
            )?;

            Fat32FileOps {
                device: self.device.clone(),
                geom: self.geom,
            }
            .write_within_sector(first_sector, 0, &me)?;

            Fat32FileOps {
                device: self.device.clone(),
                geom: self.geom,
            }
            .write_within_sector(first_sector, 32, &up)?;
        }

        let entry = Self::directory_entry(name, cluster, file_type)?;

        let (sector_number, offset) = self.free_slot(parent_cluster)?;

        Fat32FileOps {
            device: self.device.clone(),
            geom: self.geom,
        }
        .write_within_sector(sector_number, offset, &entry)?;

        let mut dir_name = [b' '; 11];
        dir_name.copy_from_slice(&entry[..11]);

        Ok(Arc::new(Mutex::new(Inode {
            id: FAT_INODE_BASE | (cluster as u64),
            cluster,
            file_type,
            size: 0,
            inode_ops: inode.inode_ops,
            file_ops: inode.file_ops,
            dentry: None,
            parent_cluster,
            dir_name,
        })))
    }

    fn mkdir(&self, inode: &mut Inode, name: &[u8]) -> Result<Arc<Mutex<Inode>>, &'static str> {
        self.create(inode, name, FileType::Directory)
    }

    fn rmdir(&self, inode: &mut Inode, name: &[u8]) -> Result<(), &'static str> {
        if inode.file_type != FileType::Directory {
            return Err("not a directory");
        }

        if name == b"." || name == b".." {
            return Err("cannot remove . or ..");
        }

        let mut entry = self.find_entry(inode.cluster, name)?;

        let high = u16::from_le_bytes([entry[20], entry[21]]) as u32;
        let low = u16::from_le_bytes([entry[26], entry[27]]) as u32;
        let cluster = (high << 16) | low;

        if entry[11] & 0x10 == 0 {
            return Err("not a directory");
        }

        if !Self::is_empty_dir(&self.device, self.geom, cluster)? {
            return Err("directory not empty");
        }

        Self::mark_deleted(&self.device, self.geom, inode.cluster, &mut entry)?;

        if cluster >= FIRST_DATA_CLUSTER {
            self.free_chain(cluster)?;
        }

        Ok(())
    }

    fn unlink(&self, inode: &mut Inode, name: &[u8]) -> Result<(), &'static str> {
        if inode.file_type != FileType::Directory {
            return Err("not a directory");
        }

        if name == b"." || name == b".." {
            return Err("cannot remove . or ..");
        }

        let mut entry = self.find_entry(inode.cluster, name)?;

        if entry[11] & 0x10 != 0 {
            return Err("is a directory");
        }

        let high = u16::from_le_bytes([entry[20], entry[21]]) as u32;
        let low = u16::from_le_bytes([entry[26], entry[27]]) as u32;
        let cluster = (high << 16) | low;

        Self::mark_deleted(&self.device, self.geom, inode.cluster, &mut entry)?;

        if cluster >= FIRST_DATA_CLUSTER {
            self.free_chain(cluster)?;
        }

        Ok(())
    }
}

// Sixteen clean advisory chain positions. Every FAT mutation clears all slots.
// No data, size or readiness cache. Device lifetime assumptions match baseline.
static CLUSTER_OFFSET_CACHE: Mutex<[Option<(usize, u32, u64, u32)>;16]> = Mutex::new([None;16]);
fn cache_chain_position(device:usize,start:u32,index:u64,cluster:u32) {
    let mut cache=CLUSTER_OFFSET_CACHE.lock();
    // Group nearby positions into one slot; retain checkpoints across a file.
    let slot=((index / 256) as usize)%cache.len();
    cache[slot]=Some((device,start,index,cluster));
}

impl<D: BlockDevice> Fat32FileOps<D> {
    fn cluster_for_offset(&self, start_cluster: u32, offset: u64) -> Result<u32, &'static str> {
        let cluster_size = self.geom.bytes_per_sector as u64 * self.geom.sectors_per_cluster as u64;

        let target_idx = offset / cluster_size;
        let mut start_idx = 0;
        let mut cluster = start_cluster;

        {
            let cache = CLUSTER_OFFSET_CACHE.lock();
            for &(c_device,c_start,c_idx,c_cluster) in cache.iter().flatten() {
                if c_device==Arc::as_ptr(&self.device) as usize && c_start==start_cluster
                    && c_idx<=target_idx && c_idx>=start_idx {
                    start_idx=c_idx;cluster=c_cluster;
                }
            }
        }

        let max_hops = self.geom.sectors_per_fat as u64 * 128;
        if target_idx.saturating_sub(start_idx) > max_hops {
            return Err("corrupt FAT chain");
        }

        for _ in start_idx..target_idx {
            cluster = match self.geom.next_cluster(&self.device, cluster)? {
                Some(next) => next,
                None => return Err("offset past EOF"),
            };
        }

        cache_chain_position(Arc::as_ptr(&self.device) as usize, start_cluster, target_idx, cluster);

        Ok(cluster)
    }
}

impl<D: BlockDevice> Fat32FileOps<D> {
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

impl<D: BlockDevice> Fat32FileOps<D> {
    fn write_within_sector(
        &self,
        sector_number: u64,
        offset_in_sector: usize,
        data: &[u8],
    ) -> Result<(), &'static str> {
        let mut sector = [0u8; MAX_SECTOR];

        read_sector(
            &self.device,
            sector_number,
            self.geom.bytes_per_sector,
            &mut sector,
        )?;

        let end = offset_in_sector + data.len();

        if end > self.geom.bytes_per_sector as usize {
            return Err("write crosses the end of a sector");
        }

        sector[offset_in_sector..end].copy_from_slice(data);

        let len = self.geom.bytes_per_sector as usize;

        write_sector(
            &self.device,
            sector_number,
            self.geom.bytes_per_sector,
            &sector[..len],
        )
    }

    fn cluster_for_write(&self, inode: &Inode, offset: u64) -> Result<u32, &'static str> {
        let cluster_size = self.geom.bytes_per_sector as u64 * self.geom.sectors_per_cluster as u64;

        if inode.cluster<FIRST_DATA_CLUSTER || inode.cluster>=self.geom.cluster_limit {
            return Err("file start cluster outside volume");
        }
        let wanted = offset / cluster_size;

        let mut cluster = inode.cluster;
        let mut index = 0u64;
        {
            let cache=CLUSTER_OFFSET_CACHE.lock();
            for &(dev,start,idx,cached) in cache.iter().flatten() {
                if dev==Arc::as_ptr(&self.device) as usize && start==inode.cluster && idx<=wanted
                    && idx>=index && cached>=FIRST_DATA_CLUSTER && cached<self.geom.cluster_limit {
                    cluster=cached;index=idx;
                }
            }
        }
        let max_hops = self.geom.sectors_per_fat as u64 * 128;

        loop {
            if index > wanted || index > max_hops {
                return Err("corrupt FAT chain");
            }

            if index == wanted {
                return Ok(cluster);
            }

            match self.geom.next_cluster(&self.device, cluster)? {
                Some(next) => {
                    cluster = next;
                    index += 1;
                }
                None => {
                    let fresh = self.geom.allocate_cluster(&self.device)?;

                    self.geom.set_fat_entry(&self.device, cluster, fresh)?;

                    cluster = fresh;
                    index += 1;
                }
            }
        }
    }
}

impl<D: BlockDevice> FileOperations for Fat32FileOps<D> {
    fn readdir(&self, inode: &Inode, offset: u64) -> Result<Option<RawEntry>, &'static str> {
        if inode.file_type != FileType::Directory {
            return Err("not a directory");
        }

        Fat32InodeOps {
            device: self.device.clone(),
            geom: self.geom,
        }
        .entry_at(inode.cluster, offset)
    }

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
            // Coalesce only complete adjacent clusters, bounded to one 4-KiB
            // device command. Every successor still goes through FAT validation.
            if cluster_offset == 0 && cluster_size <= 4096 {
                let max_clusters = ((requested - copied) / cluster_size).min(4096 / cluster_size);
                let mut run_clusters = 1usize;
                let mut last_cluster = cluster;
                while run_clusters < max_clusters {
                    match self.geom.next_cluster(&self.device, last_cluster)? {
                        Some(next) if last_cluster.checked_add(1) == Some(next) => {
                            last_cluster = next;
                            run_clusters += 1;
                        }
                        _ => break,
                    }
                }
                if run_clusters > 1 {
                    let bytes = run_clusters * cluster_size;
                    let first = self.geom.cluster_sector(cluster)?;
                    self.geom.cluster_sector(last_cluster)?;
                    self.device.lock().read_block(first, &mut buf[copied..copied + bytes])?;
                    copied += bytes;
                    position += bytes;
                    cluster = last_cluster;
                    if copied < requested {
                        cluster = self.geom.next_cluster(&self.device, cluster)?
                            .ok_or("unexpected end of FAT chain")?;
                    }
                    continue;
                }
            }
            let count = core::cmp::min(cluster_size - cluster_offset, requested - copied);

            let first_sector =
                self.geom.cluster_sector(cluster)? + (cluster_offset / bytes_per_sector) as u64;

            let sector_offset = cluster_offset % bytes_per_sector;

            let mut done = 0usize;

            // Fast path: if aligned to sector boundaries, read multiple contiguous sectors in one command
            if sector_offset == 0 {
                let whole_sectors = count / bytes_per_sector;
                let whole_bytes = whole_sectors * bytes_per_sector;
                if whole_bytes > 0 {
                    self.device
                        .lock()
                        .read_block(first_sector, &mut buf[copied..copied + whole_bytes])?;
                    done += whole_bytes;
                }
            }

            while done < count {
                let amount = core::cmp::min(
                    bytes_per_sector - (sector_offset + done) % bytes_per_sector,
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

            if copied < requested && position.is_multiple_of(cluster_size) {
                cluster = match self.geom.next_cluster(&self.device, cluster)? {
                    Some(next) => next,
                    None => return Err("unexpected end of FAT chain"),
                };
            }
        }

        Ok(copied)
    }

    fn write(&self, inode: &mut Inode, offset: u64, buf: &[u8]) -> Result<usize, &'static str> {
        if inode.file_type != FileType::Regular {
            return Err("not a regular file");
        }

        if buf.is_empty() {
            return Ok(0);
        }

        let bytes_per_sector = self.geom.bytes_per_sector as usize;
        let cluster_size = bytes_per_sector * self.geom.sectors_per_cluster as usize;

        let mut cluster = self.cluster_for_write(inode, offset)?;
        let mut position = offset as usize;
        let mut written = 0usize;

        while written < buf.len() {
            let cluster_offset = position % cluster_size;
            // Existing complete clusters only. Do not allocate speculatively or
            // skip an allocation/directory-size update at the end of the run.
            if cluster_offset == 0 && cluster_size <= 4096 {
                let remaining_existing = inode.size.saturating_sub(position as u64) as usize;
                let max_clusters = ((buf.len() - written) / cluster_size)
                    .min(remaining_existing / cluster_size).min(4096 / cluster_size);
                let mut run_clusters = 1usize;
                let mut last_cluster = cluster;
                while run_clusters < max_clusters {
                    match self.geom.next_cluster(&self.device, last_cluster)? {
                        Some(next) if last_cluster.checked_add(1) == Some(next) => {
                            last_cluster = next;
                            run_clusters += 1;
                        }
                        _ => break,
                    }
                }
                if run_clusters > 1 {
                    let bytes = run_clusters * cluster_size;
                    let first = self.geom.cluster_sector(cluster)?;
                    self.geom.cluster_sector(last_cluster)?;
                    self.device.lock().write_block(first, &buf[written..written + bytes])?;
                    written += bytes;
                    position += bytes;
                    cluster = last_cluster;
                    if written < buf.len() {
                        cluster = match self.geom.next_cluster(&self.device, cluster)? {
                            Some(next) => next,
                            None => {
                                let fresh = self.geom.allocate_cluster(&self.device)?;
                                self.geom.set_fat_entry(&self.device, cluster, fresh)?;
                                fresh
                            }
                        };
                    }
                    continue;
                }
            }
            let count = core::cmp::min(cluster_size - cluster_offset, buf.len() - written);

            let first_sector =
                self.geom.cluster_sector(cluster)? + (cluster_offset / bytes_per_sector) as u64;

            let sector_offset = cluster_offset % bytes_per_sector;

            let mut done = 0usize;

            // Fast path: whole aligned sectors inside this cluster go down as one
            // block write (the AHCI layer splits it into <=4096-byte commands and
            // skips the read-modify-write for fully covered runs). Partial head/tail
            // sectors keep the read-modify-write path below.
            if sector_offset == 0 {
                let whole_bytes = (count / bytes_per_sector) * bytes_per_sector;
                if whole_bytes > 0 {
                    self.device
                        .lock()
                        .write_block(first_sector, &buf[written..written + whole_bytes])?;
                    done += whole_bytes;
                }
            }

            while done < count {
                let amount = core::cmp::min(
                    bytes_per_sector - (sector_offset + done) % bytes_per_sector,
                    count - done,
                );

                self.write_within_sector(
                    first_sector + ((sector_offset + done) / bytes_per_sector) as u64,
                    (sector_offset + done) % bytes_per_sector,
                    &buf[written + done..written + done + amount],
                )?;

                done += amount;
            }

            written += count;
            position += count;

            if written < buf.len() && position.is_multiple_of(cluster_size) {
                cluster = match self.geom.next_cluster(&self.device, cluster)? {
                    Some(next) => next,
                    None => {
                        let fresh = self.geom.allocate_cluster(&self.device)?;

                        self.geom.set_fat_entry(&self.device, cluster, fresh)?;

                        fresh
                    }
                };
            }
        }

        let end = offset + buf.len() as u64;

        if end > inode.size {
            inode.size = end;

            self.write_directory_size(inode)?;
        }

        if written>0 {
            cache_chain_position(Arc::as_ptr(&self.device) as usize,inode.cluster,
                (position as u64-1)/cluster_size as u64,cluster);
        }
        Ok(written)
    }
}

impl<D: BlockDevice> Fat32FileOps<D> {
    fn write_directory_size(&self, inode: &Inode) -> Result<(), &'static str> {
        let parent_cluster = inode.parent_cluster;
        let target = inode.dir_name;

        if parent_cluster == 0 {
            return Ok(());
        }

        let mut cluster = parent_cluster;
        let mut hops = 0u64;
        let max_hops = self.geom.sectors_per_fat as u64 * 128;

        loop {
            let first_sector = self.geom.cluster_sector(cluster)?;

            for sector_index in 0..self.geom.sectors_per_cluster as u64 {
                let sector_number = first_sector + sector_index;

                let mut sector = [0u8; MAX_SECTOR];

                read_sector(
                    &self.device,
                    sector_number,
                    self.geom.bytes_per_sector,
                    &mut sector,
                )?;

                let len = self.geom.bytes_per_sector as usize;

                for offset in (0..len).step_by(32) {
                    let entry = &sector[offset..offset + 32];

                    if entry[0] == 0x00 {
                        return Ok(());
                    }

                    if entry[0] == 0xE5 || entry[11] == 0x0F || entry[11] & 0x08 != 0 {
                        continue;
                    }

                    if entry[..11] != target {
                        continue;
                    }

                    sector[offset + 28..offset + 32]
                        .copy_from_slice(&(inode.size as u32).to_le_bytes());

                    let cluster_high = ((inode.cluster >> 16) as u16).to_le_bytes();
                    let cluster_low = (inode.cluster as u16).to_le_bytes();
                    sector[offset + 20..offset + 22].copy_from_slice(&cluster_high);
                    sector[offset + 26..offset + 28].copy_from_slice(&cluster_low);

                    return write_sector(
                        &self.device,
                        sector_number,
                        self.geom.bytes_per_sector,
                        &sector[..len],
                    );
                }
            }

            hops += 1;
            if hops >= max_hops {
                return Err("corrupt FAT chain");
            }
            match self.geom.next_cluster(&self.device, cluster)? {
                Some(next) => cluster = next,
                None => return Ok(()),
            }
        }
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
