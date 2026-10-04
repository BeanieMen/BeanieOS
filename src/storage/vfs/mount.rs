use alloc::sync::Arc;

use spin::Mutex;

use super::dentry::Dentry;

pub trait FileSystem: Sync + Send {
    fn root(&self) -> Arc<Mutex<Dentry>>;

    fn sync(&self) -> Result<(), &'static str>;
}

pub struct Mount {
    pub root: Arc<Mutex<Dentry>>,
    pub fs: Arc<dyn FileSystem>,
    pub mountpoint: Option<Arc<Mutex<Dentry>>>,
}

impl Mount {
    pub fn new(fs: Arc<dyn FileSystem>, mountpoint: Option<Arc<Mutex<Dentry>>>) -> Self {
        let root = fs.root();

        Self {
            root,
            fs,
            mountpoint,
        }
    }
}
