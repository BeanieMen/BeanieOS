use alloc::vec;
use fatfs::{FileSystem, Read, Write};

use crate::{fs::Disk, print, println};

pub struct Shell {
    input_buffer: [u8; 256],
    input_len: usize,
    fs: FileSystem<Disk>,
    current_dir: [u8; 256],
    current_dir_len: usize,
}

impl Shell {
    pub fn new(fs: FileSystem<Disk>) -> Self {
        let mut current_dir = [0; 256];
        current_dir[0] = b'/';

        Self {
            input_buffer: [0; 256],
            input_len: 0,
            fs,
            current_dir,
            current_dir_len: 1,
        }
    }

    pub fn shell_input(&mut self, inp: char) {
        if inp == '\x08' {
            if self.input_len > 0 {
                self.input_len -= 1;
                self.input_buffer[self.input_len] = 0;
                print!("\x08 \x08");
            }
            return;
        }

        if inp == '\n' {
            println!();

            let input = self.input_buffer[..self.input_len].to_vec();
            let input = input.trim_ascii();

            if input == b"help" {
                self.help();
            } else if input == b"test-file" {
                self.test_file();
            } else if let Some(path) = input.strip_prefix(b"ls") {
                let path = path.trim_ascii_start();

                if path.is_empty() {
                    self.ls_current();
                } else if let Ok(path) = core::str::from_utf8(path) {
                    self.ls(path);
                } else {
                    println!("Invalid path");
                }
            } else if let Some(path) = input.strip_prefix(b"cd") {
                let path = path.trim_ascii_start();

                let path = if path.is_empty() {
                    "/"
                } else {
                    match core::str::from_utf8(path) {
                        Ok(path) => path,
                        Err(_) => {
                            println!("Invalid path");
                            self.reset_input();
                            print!("> ");
                            return;
                        }
                    }
                };

                self.cd(path);
            } else if let Some(path) = input.strip_prefix(b"cat") {
                let path = path.trim_ascii_start();

                if path.is_empty() {
                    println!("Usage: cat <path>");
                } else if let Ok(path) = core::str::from_utf8(path) {
                    self.cat(path);
                } else {
                    println!("Invalid path");
                }
            } else if let Some(name) = input.strip_prefix(b"mkdir") {
                let name = name.trim_ascii_start();

                if name.is_empty() {
                    println!("Usage: mkdir <name>");
                } else if let Ok(name) = core::str::from_utf8(name) {
                    self.mkdir(name);
                } else {
                    println!("Invalid name");
                }
            } else if let Some(name) = input.strip_prefix(b"touch") {
                let name = name.trim_ascii_start();

                if name.is_empty() {
                    println!("Usage: touch <name>");
                } else if let Ok(name) = core::str::from_utf8(name) {
                    self.touch(name);
                } else {
                    println!("Invalid name");
                }
            } else if let Some(name) = input.strip_prefix(b"rm") {
                let name = name.trim_ascii_start();

                if name.is_empty() {
                    println!("Usage: rm <name>");
                } else if let Ok(name) = core::str::from_utf8(name) {
                    self.rm(name);
                } else {
                    println!("Invalid name");
                }
            } else if let Some(name) = input.strip_prefix(b"rmdir") {
                let name = name.trim_ascii_start();

                if name.is_empty() {
                    println!("Usage: rmdir <name>");
                } else if let Ok(name) = core::str::from_utf8(name) {
                    self.rmdir(name);
                } else {
                    println!("Invalid name");
                }
            } else if !input.is_empty() {
                println!("Unknown command");
            }

            self.reset_input();
            print!("> ");
        } else if self.input_len < self.input_buffer.len() {
            print!("{inp}");
            self.input_buffer[self.input_len] = inp as u8;
            self.input_len += 1;
        }
    }

    fn reset_input(&mut self) {
        self.input_buffer = [0; 256];
        self.input_len = 0;
    }

    fn current_path(&self) -> &str {
        core::str::from_utf8(&self.current_dir[..self.current_dir_len]).unwrap()
    }

    fn resolve_path(&self, path: &str) -> Option<([u8; 256], usize)> {
        let mut full_path = [0u8; 256];

        if path.starts_with('/') {
            let bytes = path.as_bytes();

            if bytes.len() >= full_path.len() {
                println!("Path too long");
                return None;
            }

            full_path[..bytes.len()].copy_from_slice(bytes);

            return Some((full_path, bytes.len()));
        }

        let current = &self.current_dir[..self.current_dir_len];
        let bytes = path.as_bytes();

        let mut len = self.current_dir_len;

        full_path[..len].copy_from_slice(current);

        if len > 1 {
            full_path[len] = b'/';
            len += 1;
        }

        if len + bytes.len() >= full_path.len() {
            println!("Path too long");
            return None;
        }

        full_path[len..len + bytes.len()].copy_from_slice(bytes);

        Some((full_path, len + bytes.len()))
    }

    pub fn ls_current(&self) {
        self.ls(self.current_path());
    }

    pub fn ls(&self, path: &str) {
        let root = self.fs.root_dir();

        if path == "/" {
            for file in root.iter() {
                if let Ok(file) = file {
                    println!("{}", file.file_name());
                }
            }

            return;
        }

        let dir = match root.open_dir(path) {
            Ok(dir) => dir,
            Err(_) => {
                println!("Dir not found: {}", path);
                return;
            }
        };

        for file in dir.iter() {
            if let Ok(file) = file {
                println!("{}", file.file_name());
            }
        }
    }

    pub fn cd(&mut self, path: &str) {
        if path == "/" {
            self.current_dir = [0; 256];
            self.current_dir[0] = b'/';
            self.current_dir_len = 1;
            return;
        }

        let (full_path, full_path_len) = match self.resolve_path(path) {
            Some(path) => path,
            None => return,
        };

        let full_path_str =
            core::str::from_utf8(&full_path[..full_path_len]).unwrap();

        if self.fs.root_dir().open_dir(full_path_str).is_err() {
            println!("Directory not found: {}", path);
            return;
        }

        println!("Changed directory to {}", full_path_str);

        self.current_dir = full_path;
        self.current_dir_len = full_path_len;
    }

    pub fn cat(&self, path: &str) {
        let (full_path, full_path_len) = match self.resolve_path(path) {
            Some(path) => path,
            None => return,
        };

        let full_path_str =
            core::str::from_utf8(&full_path[..full_path_len]).unwrap();

        let root = self.fs.root_dir();

        let mut file = match root.open_file(full_path_str) {
            Ok(file) => file,
            Err(_) => {
                println!("File not found: {}", path);
                return;
            }
        };

        let mut buffer = vec![0u8; 512];

        loop {
            let bytes_read = match file.read(&mut buffer) {
                Ok(n) => n,
                Err(_) => {
                    println!("Read error");
                    return;
                }
            };

            if bytes_read == 0 {
                break;
            }

            for byte in &buffer[..bytes_read] {
                print!("{}", *byte as char);
            }
        }

        println!();
    }

    pub fn test_file(&self) {
        let root = self.fs.root_dir();

        println!("Creating test file in {}", self.current_path());

        if self.current_path() == "/" {
            let mut file = match root.create_file("test.txt") {
                Ok(file) => file,
                Err(_) => {
                    println!("Failed to create test.txt");
                    return;
                }
            };

            if file.write_all(b"meow").is_err() {
                println!("Failed to write test.txt");
                return;
            }

            if file.flush().is_err() {
                println!("Failed to flush test.txt");
                return;
            }

            println!("Created test.txt");
            return;
        }

        let dir = match root.open_dir(self.current_path()) {
            Ok(dir) => dir,
            Err(_) => {
                println!("Directory not found");
                return;
            }
        };

        let mut file = match dir.create_file("test.txt") {
            Ok(file) => file,
            Err(_) => {
                println!("Failed to create test.txt");
                return;
            }
        };

        if file.write_all(b"meow").is_err() {
            println!("Failed to write test.txt");
            return;
        }

        if file.flush().is_err() {
            println!("Failed to flush test.txt");
            return;
        }

        println!("Created test.txt");
    }

    pub fn mkdir(&self, name: &str) {
        let root = self.fs.root_dir();

        if self.current_path() == "/" {
            if root.create_dir(name).is_err() {
                println!("Failed to create directory: {}", name);
                return;
            }

            println!("Created directory: {}", name);
            return;
        }

        let dir = match root.open_dir(self.current_path()) {
            Ok(dir) => dir,
            Err(_) => {
                println!("Directory not found: {}", self.current_path());
                return;
            }
        };

        if dir.create_dir(name).is_err() {
            println!("Failed to create directory: {}", name);
            return;
        }

        println!("Created directory: {}", name);
    }

    pub fn touch(&self, name: &str) {
        let root = self.fs.root_dir();

        if self.current_path() == "/" {
            if root.create_file(name).is_err() {
                println!("Failed to create file: {}", name);
                return;
            }

            println!("Created file: {}", name);
            return;
        }

        let dir = match root.open_dir(self.current_path()) {
            Ok(dir) => dir,
            Err(_) => {
                println!("Directory not found: {}", self.current_path());
                return;
            }
        };

        if dir.create_file(name).is_err() {
            println!("Failed to create file: {}", name);
            return;
        }

        println!("Created file: {}", name);
    }

    pub fn rm(&self, name: &str) {
        let root = self.fs.root_dir();

        if self.current_path() == "/" {
            if root.remove(name).is_err() {
                println!("Failed to remove file: {}", name);
                return;
            }

            println!("Removed file: {}", name);
            return;
        }

        let dir = match root.open_dir(self.current_path()) {
            Ok(dir) => dir,
            Err(_) => {
                println!("Directory not found: {}", self.current_path());
                return;
            }
        };

        if dir.remove(name).is_err() {
            println!("Failed to remove file: {}", name);
            return;
        }

        println!("Removed file: {}", name);
    }

    pub fn rmdir(&self, name: &str) {
        let root = self.fs.root_dir();

        if self.current_path() == "/" {
            if root.remove(name).is_err() {
                println!("Failed to remove directory: {}", name);
                return;
            }

            println!("Removed directory: {}", name);
            return;
        }

        let dir = match root.open_dir(self.current_path()) {
            Ok(dir) => dir,
            Err(_) => {
                println!("Directory not found: {}", self.current_path());
                return;
            }
        };

        if dir.remove(name).is_err() {
            println!("Failed to remove directory: {}", name);
            return;
        }

        println!("Removed directory: {}", name);
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