use alloc::{string::String, sync::Arc};

use spin::Mutex;

use super::{dentry::Dentry, inode::Inode};

pub struct Path {
    pub dentry: Arc<Mutex<Dentry>>,
}

impl Path {
    pub fn new(dentry: Arc<Mutex<Dentry>>) -> Self {
        Self { dentry }
    }

    pub fn inode(&self) -> Arc<Mutex<Inode>> {
        self.dentry.lock().inode.clone()
    }

    pub fn parent(&self) -> Option<Arc<Mutex<Dentry>>> {
        self.dentry.lock().parent.clone()
    }

    pub fn name(&self) -> String {
        self.dentry.lock().name.clone()
    }

    pub fn resolve(root: Arc<Mutex<Dentry>>, path: &[u8]) -> Result<Self, &'static str> {
        if path.is_empty() {
            return Err("empty path");
        }

        if path[0] != b'/' {
            return Err("relative path");
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

            current = Arc::new(Mutex::new(Dentry::new(name, child, Some(current.clone()))));
        }

        Ok(Self::new(current))
    }
}
