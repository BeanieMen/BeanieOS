pub mod directory;
pub mod file;
pub mod storage;

use fatfs::{FileSystem as FatFileSystem, FsOptions};

use crate::fs::storage::block::BlockDevice;

pub struct FileSystem<D: BlockDevice> {
    inner: FatFileSystem<FatStorage<D>>,
}

impl<D: BlockDevice> FileSystem<D> {
pub fn mount(device: D) -> Result<Self, fatfs::Error<()>> {
    let storage = FatStorage::new(device);

    let Ok(inner) = FatFileSystem::new(storage, FsOptions::new()) else {
        return Err(fatfs::Error::Io(()));
    };

    Ok(Self { inner })
}
}

pub struct FatStorage<D: BlockDevice> {
    device: D,
    position: u64,
}

impl<D: BlockDevice> FatStorage<D> {
    pub fn new(device: D) -> Self {
        Self {
            device,
            position: 0,
        }
    }
}

impl<D: BlockDevice> fatfs::IoBase for FatStorage<D> {
    type Error = fatfs::Error<()>;
}

impl<D: BlockDevice> fatfs::Read for FatStorage<D> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut total = 0;

        while total < buf.len() {
            let sector = self.position / storage::block::SECTOR_SIZE as u64;
            let offset = (self.position % storage::block::SECTOR_SIZE as u64) as usize;
            let mut sector_buf = [0u8; storage::block::SECTOR_SIZE];
            self.device
                .read_sector(sector, &mut sector_buf)
                .map_err(|_| fatfs::Error::Io(()))?;

            let count = (storage::block::SECTOR_SIZE - offset).min(buf.len() - total);

            buf[total..total + count].copy_from_slice(&sector_buf[offset..offset + count]);
            total += count;
            self.position += count as u64;
        }

        Ok(total)
    }
}

impl<D: BlockDevice> fatfs::Write for FatStorage<D> {
    fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        let mut total = 0;

        while total < buffer.len() {
            let sector = self.position / storage::block::SECTOR_SIZE as u64;
            let offset = self.position as usize % storage::block::SECTOR_SIZE;

            let mut sector_buffer = [0u8; storage::block::SECTOR_SIZE];

            self.device
                .read_sector(sector, &mut sector_buffer)
                .map_err(|_| fatfs::Error::Io(()))?;

            let count = (storage::block::SECTOR_SIZE - offset).min(buffer.len() - total);

            sector_buffer[offset..offset + count].copy_from_slice(&buffer[total..total + count]);

            self.device
                .write_sector(sector, &sector_buffer)
                .map_err(|_| fatfs::Error::Io(()))?;

            total += count;
            self.position += count as u64;
        }

        Ok(total)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<D: BlockDevice> fatfs::Seek for FatStorage<D> {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let new_position = match pos {
            fatfs::SeekFrom::Start(offset) => offset,
            fatfs::SeekFrom::End(offset) => {
                let size = self.device.sector_count() * storage::block::SECTOR_SIZE as u64;
                if offset < 0 {
                    size.checked_sub((-offset) as u64)
                        .ok_or(fatfs::Error::Io(()))?
                } else {
                    size.checked_add(offset as u64)
                        .ok_or(fatfs::Error::Io(()))?
                }
            }
            fatfs::SeekFrom::Current(offset) => {
                if offset < 0 {
                    self.position
                        .checked_sub((-offset) as u64)
                        .ok_or(fatfs::Error::Io(()))?
                } else {
                    self.position
                        .checked_add(offset as u64)
                        .ok_or(fatfs::Error::Io(()))?
                }
            }
        };

        self.position = new_position;
        Ok(new_position)
    }
}
