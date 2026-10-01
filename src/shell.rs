use core::str;

use fatfs::{DefaultTimeProvider, Dir, FileSystem, LossyOemCpConverter, Read, Write};
use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::fs::Disk;
use crate::graphics::framebuffer::WRITER;
use crate::{print, println};

const INPUT_MAX: usize = 256;
const PATH_MAX: usize = 256;

/// Keys the keyboard has produced and nothing has read yet. The interrupt
/// handler writes here and the main loop drains it, so no filesystem work or
/// printing happens with interrupts off.
const PENDING_MAX: usize = 64;

static PENDING: Mutex<[char; PENDING_MAX]> = Mutex::new(['\0'; PENDING_MAX]);

pub fn push_key(key: char) {
    interrupts::without_interrupts(|| {
        let mut ring = PENDING.lock();
        if let Some(slot) = ring.iter_mut().find(|slot| **slot == '\0') {
            *slot = key;
        }
    })
}

pub fn pop_key() -> Option<char> {
    interrupts::without_interrupts(|| {
        let mut ring = PENDING.lock();
        let slot = ring.iter_mut().find(|slot| **slot != '\0')?;
        let key = *slot;
        *slot = '\0';
        Some(key)
    })
}

type FatDir<'a> = Dir<'a, Disk, DefaultTimeProvider, LossyOemCpConverter>;

#[derive(Clone)]
struct Path {
    bytes: [u8; PATH_MAX],
    len: usize,
}

impl Path {
    const fn root() -> Self {
        let mut bytes = [0; PATH_MAX];
        bytes[0] = b'/';
        Path { bytes, len: 1 }
    }

    fn as_str(&self) -> &str {
        str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }

    fn push(&mut self, more: &str) -> bool {
        let add = more.as_bytes();

        if self.len + add.len() > PATH_MAX {
            return false;
        }

        self.bytes[self.len..self.len + add.len()].copy_from_slice(add);
        self.len += add.len();
        true
    }

    fn pop_component(&mut self) {
        if self.len <= 1 {
            return;
        }

        while self.len > 1 && self.bytes[self.len - 1] != b'/' {
            self.len -= 1;
        }

        if self.len > 1 {
            self.len -= 1;
        }
    }
}

pub struct Shell {
    input: [u8; INPUT_MAX],
    input_len: usize,
    fs: FileSystem<Disk>,
    cwd: Path,
}

impl Shell {
    pub fn new(fs: FileSystem<Disk>) -> Self {
        Shell {
            input: [0; INPUT_MAX],
            input_len: 0,
            fs,
            cwd: Path::root(),
        }
    }

    pub fn shell_input(&mut self, c: char) {
        match c {
            '\x08' => {
                if self.input_len > 0 {
                    self.input_len -= 1;
                    print!("\x08");
                }
            }

            '\n' => {
                println!();
                self.submit();
            }

            _ if self.input_len < INPUT_MAX => {
                print!("{c}");
                self.input[self.input_len] = c as u8;
                self.input_len += 1;
            }

            _ => {}
        }
    }

    fn submit(&mut self) {
        let mut line = [0; INPUT_MAX];
        line[..self.input_len].copy_from_slice(&self.input[..self.input_len]);
        let line = str::from_utf8(&line[..self.input_len]).unwrap_or("");

        let mut words = line.split_whitespace();
        let command = words.next().unwrap_or("");
        let arg = words.next().unwrap_or("");

        self.run(command, arg);

        self.input_len = 0;
        print!("> ");
    }

    fn run(&mut self, command: &str, arg: &str) {
        match command {
            "" => {}

            "help" => self.help(),

            "ls" => self.ls(arg),

            "cd" => self.cd(if arg.is_empty() { "/" } else { arg }),

            "cat" => match arg.is_empty() {
                true => println!("Usage: cat <path>"),
                false => self.cat(arg),
            },

            "mkdir" => match arg.is_empty() {
                true => println!("Usage: mkdir <name>"),
                false => self.mkdir(arg),
            },

            "touch" => match arg.is_empty() {
                true => println!("Usage: touch <name>"),
                false => self.touch(arg),
            },

            "rm" => match arg.is_empty() {
                true => println!("Usage: rm <name>"),
                false => self.rm(arg),
            },

            "rmdir" => match arg.is_empty() {
                true => println!("Usage: rmdir <name>"),
                false => self.rmdir(arg),
            },

            "test-file" => self.test_file(),

            other => println!("Unknown command: {other}"),
        }
    }

    fn resolve(&self, path: &str) -> Option<Path> {
        if path == "/" {
            return Some(Path::root());
        }

        let (base, rest) = if let Some(rest) = path.strip_prefix('/') {
            (Path::root(), rest)
        } else {
            (self.cwd.clone(), path)
        };

        let mut full = base;

        for part in rest.split('/') {
            match part {
                "" | "." => {}
                ".." => full.pop_component(),
                name => {
                    if full.len > 1 && !full.push("/") {
                        return None;
                    }

                    if !full.push(name) {
                        return None;
                    }
                }
            }
        }

        Some(full)
    }
    
    fn cwd_dir(&self) -> FatDir<'_> {
        let root = self.fs.root_dir();
        let path = self.cwd.as_str();

        if path == "/" {
            return root;
        }

        root.open_dir(path).unwrap_or(root)
    }

    pub fn ls(&self, path: &str) {
        let dir = if path.is_empty() {
            self.cwd_dir()
        } else {
            let Some(full) = self.resolve(path) else {
                println!("Path too long");
                return;
            };

            match self.fs.root_dir().open_dir(full.as_str()) {
                Ok(dir) => dir,
                Err(_) => {
                    println!("Directory not found: {path}");
                    return;
                }
            }
        };

        for entry in dir.iter() {
            if let Ok(entry) = entry {
                println!("{}", entry.file_name());
            }
        }
    }

    pub fn cd(&mut self, path: &str) {
        let Some(full) = self.resolve(path) else {
            println!("Path too long");
            return;
        };

        if full.as_str() != "/" && self.fs.root_dir().open_dir(full.as_str()).is_err() {
            println!("Directory not found: {path}");
            return;
        }

        println!("Changed directory to {}", full.as_str());
        self.cwd = full;
    }

    pub fn cat(&self, path: &str) {
        let Some(full) = self.resolve(path) else {
            println!("Path too long");
            return;
        };

        let mut file = match self.fs.root_dir().open_file(full.as_str()) {
            Ok(file) => file,
            Err(_) => {
                println!("File not found: {path}");
                return;
            }
        };

        let mut buffer = [0u8; 512];

        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    WRITER.lock().write_bytes(&buffer[..n]);
                    println!();
                }
                Err(_) => {
                    println!("Read error");
                    return;
                }
            }
        }
    }

    pub fn mkdir(&self, name: &str) {
        if self.cwd_dir().create_dir(name).is_err() {
            println!("Failed to create directory: {name}");
        } else {
            println!("Created directory: {name}");
        }
    }

    pub fn touch(&self, name: &str) {
        if self.cwd_dir().create_file(name).is_err() {
            println!("Failed to create file: {name}");
        } else {
            println!("Created file: {name}");
        }
    }

    pub fn rm(&self, name: &str) {
        if self.cwd_dir().remove(name).is_err() {
            println!("Failed to remove file: {name}");
        } else {
            println!("Removed file: {name}");
        }
    }

    pub fn rmdir(&self, name: &str) {
        if self.cwd_dir().remove(name).is_err() {
            println!("Failed to remove directory: {name}");
        } else {
            println!("Removed directory: {name}");
        }
    }

    pub fn test_file(&self) {
        let mut file = match self.cwd_dir().create_file("test.txt") {
            Ok(file) => file,
            Err(_) => {
                println!("Failed to create test.txt");
                return;
            }
        };

        if file.write_all(b"meow").is_err() || file.flush().is_err() {
            println!("Failed to write test.txt");
            return;
        }

        println!("Created test.txt");
    }

    pub fn help(&self) {
        println!("Available commands:");
        println!("  help - Show this help message");
        println!("  cd <path> - Change the current directory");
        println!("  ls - List files in the current directory");
        println!("  ls <path> - List files in a directory");
        println!("  cat <path> - Display the contents of a file");
        println!("  mkdir <name> - Create a directory");
        println!("  touch <name> - Create a file");
        println!("  rm <name> - Remove a file");
        println!("  rmdir <name> - Remove a directory");
        println!("  test-file - Create a test file in current directory");
    }
}
