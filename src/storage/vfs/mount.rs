use alloc::{sync::Arc, vec::Vec};

use spin::Mutex;

use super::dentry::Dentry;
use super::path::Path;

pub(crate) trait FileSystem: Sync + Send {
    fn root(&self) -> Arc<Mutex<Dentry>>;

    fn sync(&self) -> Result<(), &'static str>;
}

struct Mount {
    root: Arc<Mutex<Dentry>>,
    #[allow(dead_code)]
    fs: Arc<dyn FileSystem>,
    // Directory this filesystem is attached to. `None` is the root mount, which
    // every path starts inside.
    mountpoint: Option<Arc<Mutex<Dentry>>>,
}

impl Mount {
    fn new(fs: Arc<dyn FileSystem>, mountpoint: Option<Arc<Mutex<Dentry>>>) -> Self {
        let root = fs.root();

        Self {
            root,
            fs,
            mountpoint,
        }
    }

    // Leading path bytes this mount claims. The root mount claims everything, so it
    // must lose to a real mountpoint that is a longer prefix.
    fn prefix_len(&self) -> usize {
        match &self.mountpoint {
            None => 0,
            Some(dentry) => dentry.lock().name.len() + 1,
        }
    }

    fn matches(&self, path: &[u8]) -> bool {
        match &self.mountpoint {
            None => true,
            Some(dentry) => {
                let name = dentry.lock().name.clone();

                path.len() > name.len()
                    && path.starts_with(name.as_bytes())
                    && path[name.len()] == b'/'
            }
        }
    }
}

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

        Path::resolve(mount.root.clone(), path)
    }

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
