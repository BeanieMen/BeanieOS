use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;

use crate::storage::vfs::inode::Inode;

pub(crate) struct Dentry {
    pub name: String,
    pub inode: Arc<Mutex<Inode>>,
    pub parent: Option<Arc<Mutex<Dentry>>>,
}

impl Dentry {
    pub(crate) fn new(
        name: String,
        inode: Arc<Mutex<Inode>>,
        parent: Option<Arc<Mutex<Dentry>>>,
    ) -> Self {
        Dentry {
            name,
            inode,
            parent,
        }
    }

    pub(crate) fn inode(&self) -> Arc<Mutex<Inode>> {
        self.inode.clone()
    }

    pub(crate) fn parent(&self) -> Option<Arc<Mutex<Dentry>>> {
        self.parent.clone()
    }
}
