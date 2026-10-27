use alloc::{string::String, sync::Arc, vec::Vec};

use spin::Mutex;

use super::{dentry::Dentry, inode::Inode};

type CacheEntry = (usize, Vec<u8>, Arc<Mutex<Dentry>>);
static RESOLVE_CACHE: Mutex<Vec<CacheEntry>> = Mutex::new(Vec::new());

pub(crate) fn invalidate_resolve_cache() {
    RESOLVE_CACHE.lock().clear();
}

pub(crate) struct Path {
    pub dentry: Arc<Mutex<Dentry>>,
}

impl Path {
    pub(crate) fn new(dentry: Arc<Mutex<Dentry>>) -> Self {
        Self { dentry }
    }

    pub(crate) fn inode(&self) -> Arc<Mutex<Inode>> {
        self.dentry.lock().inode.clone()
    }

    pub(crate) fn resolve(root: Arc<Mutex<Dentry>>, path: &[u8]) -> Result<Self, &'static str> {
        if path.is_empty() {
            return Err("empty path");
        }

        if path[0] != b'/' {
            return Err("relative path");
        }

        let root_key = Arc::as_ptr(&root) as usize;
        if let Some(dentry) = RESOLVE_CACHE
            .lock()
            .iter()
            .find(|(r, p, _)| *r == root_key && p.as_slice() == path)
            .map(|(_, _, d)| d.clone())
        {
            return Ok(Self::new(dentry));
        }

        let mut current = root;

        for component in path
            .split(|b| *b == b'/')
            .filter(|component| !component.is_empty())
        {
            if component == b"." {
                continue;
            }

            if component == b".." {
                let parent = {
                    let dentry = current.lock();
                    dentry.parent.clone()
                };

                if let Some(parent) = parent {
                    current = parent;
                }

                continue;
            }

            let inode = current.lock().inode.clone();

            let child = {
                let inode = inode.lock();

                inode.inode_ops.lookup(&inode, component)?
            };

            let name = String::from_utf8_lossy(component).into_owned();

            let dentry = Arc::new(Mutex::new(Dentry::new(
                name,
                child.clone(),
                Some(current.clone()),
            )));

            child.lock().dentry = Some(alloc::sync::Arc::downgrade(&dentry));

            current = dentry;
        }

        {
            let mut cache = RESOLVE_CACHE.lock();
            if cache.len() >= 32 {
                cache.remove(0);
            }
            cache.push((root_key, path.to_vec(), current.clone()));
        }

        Ok(Self::new(current))
    }
}
