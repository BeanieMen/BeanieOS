use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use fatfs::{FileSystem, Seek, SeekFrom, Write};
use spin::Mutex;

use crate::fs::BlockDevice;
use crate::print;

pub type Fs = FileSystem<Box<dyn BlockDevice>>;

pub const STDIN: u64 = 0;
pub const STDOUT: u64 = 1;
pub const STDERR: u64 = 2;

/// Highest descriptor handed out before the table refuses to grow.
const MAX_FDS: usize = 64;

enum Entry {
    Console,
    File { path: String, offset: u64 },
}

pub struct Fds {
    entries: Vec<Option<Entry>>,
    fs: Option<Fs>,
}

static FDS: Mutex<Fds> = Mutex::new(Fds {
    entries: Vec::new(),
    fs: None,
});

impl Fds {
    /// Adopts the mounted filesystem and installs the three standard streams.
    pub fn install(&mut self, fs: Fs) {
        self.fs = Some(fs);

        for _ in [STDIN, STDOUT, STDERR] {
            self.entries.push(Some(Entry::Console));
        }
    }

    fn entry(&mut self, fd: u64) -> Result<&mut Entry, &'static str> {
        self.entries
            .get_mut(fd as usize)
            .and_then(Option::as_mut)
            .ok_or("bad file descriptor")
    }

    /// Opens `path`, creating or truncating it when `create` is set.
    pub fn open(&mut self, path: &str, create: bool) -> Result<u64, &'static str> {
        let fs = self.fs.as_ref().ok_or("no filesystem mounted")?;

        if self.entries.len() >= MAX_FDS {
            return Err("too many open files");
        }

        if create {
            // Creating truncates, so a rewrite does not leave the tail of the old
            // contents behind.
            let _ = fs.root_dir().remove(path);

            fs.root_dir()
                .create_file(path)
                .map_err(|_| "cannot create file")?;
        } else {
            fs.root_dir().open_file(path).map_err(|_| "no such file")?;
        }

        self.entries.push(Some(Entry::File {
            path: String::from(path),
            offset: 0,
        }));

        Ok((self.entries.len() - 1) as u64)
    }

    pub fn close(&mut self, fd: u64) -> Result<(), &'static str> {
        let slot = self
            .entries
            .get_mut(fd as usize)
            .filter(|slot| slot.is_some())
            .ok_or("bad file descriptor")?;

        *slot = None;

        Ok(())
    }

    /// Writes every byte to `fd`, advancing the offset when it names a file.
    pub fn write(&mut self, fd: u64, bytes: &[u8]) -> Result<usize, &'static str> {
        // Borrow the two fields separately: the entry and the filesystem it names.
        let Some(entry) = self.entries.get_mut(fd as usize) else {
            return Err("bad file descriptor");
        };

        let Entry::File { path, offset } = entry.as_mut().ok_or("bad file descriptor")? else {
            print!("{}", String::from_utf8_lossy(bytes));

            return Ok(bytes.len());
        };

        let fs = self.fs.as_ref().ok_or("no filesystem mounted")?;
        let mut file = fs.root_dir().open_file(path).map_err(|_| "file is gone")?;

        file.seek(SeekFrom::Start(*offset))
            .map_err(|_| "cannot seek")?;

        let written = file.write(bytes).map_err(|_| "write failed")?;

        *offset += written as u64;

        Ok(written)
    }
}

pub fn install(fs: Fs) {
    FDS.lock().install(fs);
}

pub fn open(path: &str, create: bool) -> Result<u64, &'static str> {
    FDS.lock().open(path, create)
}

pub fn close(fd: u64) -> Result<(), &'static str> {
    FDS.lock().close(fd)
}

pub fn write(fd: u64, bytes: &[u8]) -> Result<usize, &'static str> {
    FDS.lock().write(fd, bytes)
}

/// Copies a NUL-terminated path out of a userspace pointer.
///
/// # Safety
///
/// `ptr` must be readable for as long as it takes to reach the terminator.
pub unsafe fn path_from_user(ptr: u64) -> Result<String, &'static str> {
    if ptr == 0 {
        return Err("null path");
    }

    let mut out = String::new();
    let mut at = ptr;

    loop {
        if out.len() >= 512 {
            return Err("path too long");
        }

        let byte = unsafe { *(at as *const u8) };
        at += 1;

        if byte == 0 {
            return Ok(out);
        }

        out.push(byte as char);
    }
}
