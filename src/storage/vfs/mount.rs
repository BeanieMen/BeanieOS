use alloc::{sync::Arc, vec::Vec};

use spin::Mutex;

use super::dentry::Dentry;
use super::path::Path;

pub(crate) trait FileSystem: Sync + Send {
    fn root(&self) -> Arc<Mutex<Dentry>>;

    fn sync(&self) -> Result<(), &'static str>;
}

#[derive(Clone)]
struct Mount {
    root: Arc<Mutex<Dentry>>,
    #[allow(dead_code)]
    fs: Arc<dyn FileSystem>,
    mountpoint: Option<Arc<Mutex<Dentry>>>,
    prefix: Option<Vec<u8>>,
    prefix_len: usize,
}

impl Mount {
    fn new(fs: Arc<dyn FileSystem>, mountpoint: Option<Arc<Mutex<Dentry>>>) -> Self {
        let root = fs.root();
        let (prefix, prefix_len) = match &mountpoint {
            None => (None, 0),
            Some(dentry) => {
                let bytes = dentry.lock().name.as_bytes().to_vec();
                let len = bytes.len() + 1;
                (Some(bytes), len)
            }
        };

        Self {
            root,
            fs,
            mountpoint,
            prefix,
            prefix_len,
        }
    }

    #[inline(always)]
    fn prefix_len(&self) -> usize {
        self.prefix_len
    }

    #[inline(always)]
    fn matches(&self, path: &[u8]) -> bool {
        match &self.prefix {
            None => true,
            Some(name) => {
                path == name.as_slice()
                    || (path.len() > name.len()
                        && path.starts_with(name)
                        && path[name.len()] == b'/')
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct MountTable {
    mounts: Vec<Mount>,
}

impl MountTable {
    pub(crate) const fn new() -> Self {
        MountTable { mounts: Vec::new() }
    }

    pub(crate) fn mount(
        &mut self,
        fs: Arc<dyn FileSystem>,
        mountpoint: Option<Arc<Mutex<Dentry>>>,
    ) {
        self.mounts.push(Mount::new(fs, mountpoint));
    }

    pub(crate) fn len(&self) -> usize {
        self.mounts.len()
    }

    pub(crate) fn resolve(&self, path: &[u8]) -> Result<Path, &'static str> {
        if path.is_empty() {
            return Err("empty path");
        }

        if path[0] != b'/' {
            return Err("relative path");
        }

        let mount = self
            .mounts
            .iter()
            .filter(|mount| mount.matches(path))
            .max_by_key(|mount| mount.prefix_len())
            .ok_or("no filesystem mounted")?;

        let sub_path = if mount.prefix_len() == 0 {
            path
        } else {
            let rem = &path[mount.prefix_len()..];
            if rem.is_empty() {
                b"/"
            } else {
                rem
            }
        };

        Path::resolve(mount.root.clone(), sub_path)
    }

    #[allow(dead_code)]
    pub(crate) fn sync_all(&self) -> Result<(), &'static str> {
        for mount in &self.mounts {
            mount.fs.sync()?;
        }

        Ok(())
    }
}

impl Default for MountTable {
    fn default() -> Self {
        Self::new()
    }
}
