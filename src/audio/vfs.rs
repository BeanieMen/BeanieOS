use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    audio::AC97_DEV,
    storage::vfs::{
        file::{File, OpenFlags},
        inode::{FileOperations, FileType, Inode},
    },
};

pub struct DspFileOps;
impl FileOperations for DspFileOps {
    fn write(
        &self,
        _inode: &mut crate::storage::vfs::inode::Inode,
        _offset: u64,
        buf: &[u8],
    ) -> Result<usize, &'static str> {
        if let Some(ref mut dev) = *AC97_DEV.lock() {
            Ok(dev.write_samples(buf))
        } else {
            Ok(buf.len())
        }
    }

    fn read(
        &self,
        _inode: &crate::storage::vfs::inode::Inode,
        _offset: u64,
        _buf: &mut [u8],
    ) -> Result<usize, &'static str> {
        Ok(0)
    }
    fn readdir(
        &self,
        _inode: &crate::storage::vfs::inode::Inode,
        _offset: u64,
    ) -> Result<Option<crate::storage::vfs::inode::RawEntry>, &'static str> {
        Err("not a directory")
    }
}

pub static DSP_INODE_OPS: crate::graphics::vfs::FbInodeOps = crate::graphics::vfs::FbInodeOps;
pub static DSP_FILE_OPS: DspFileOps = DspFileOps;

pub fn create_dsp_file(flags: OpenFlags) -> File {
    let inode = Inode {
        id: 3,
        file_type: FileType::Device,
        inode_ops: &DSP_INODE_OPS,
        file_ops: &DSP_FILE_OPS,
        size: 0,
        cluster: 0,
        dentry: None,
    };
    File::new(Arc::new(Mutex::new(inode)), flags)
}
