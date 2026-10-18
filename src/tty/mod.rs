use crate::arch::lock::InterruptMutex;
use alloc::collections::VecDeque;

pub mod vfs;

pub const TCGETS: u64 = 0x5401;
pub const TCSETS: u64 = 0x5402;
pub const TCSETSW: u64 = 0x5403;
pub const TCSETSF: u64 = 0x5404;
pub const TIOCGWINSZ: u64 = 0x5413;
pub const TIOCGPGRP: u64 = 0x540F;
pub const TIOCSPGRP: u64 = 0x5410;

pub const ICANON: u32 = 0o000002;
pub const ECHO: u32 = 0o000010;
pub const ISIG: u32 = 0o000001;

pub const VINTR: usize = 0; // Control character index for Ctrl+C

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 32],
}

impl Termios {
    pub const fn default() -> Self {
        let mut cc = [0u8; 32];
        cc[VINTR] = 0x03; // Ctrl+C = 0x03
        Self {
            c_iflag: 0,
            c_oflag: 0,
            c_cflag: 0,
            c_lflag: ISIG,
            c_line: 0,
            c_cc: cc,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

pub struct Tty {
    pub termios: Termios,
    pub line_buf: [u8; 4096],
    pub line_len: usize,
    pub queue: VecDeque<u8>,
    pub foreground_pgrp: Option<crate::task::identity::ProcessId>,
}

impl Tty {
    pub const fn new() -> Self {
        Self {
            termios: Termios::default(),
            line_buf: [0; 4096],
            line_len: 0,
            queue: VecDeque::new(),
            foreground_pgrp: None,
        }
    }

    pub fn push_byte(&mut self, byte: u8) {
        let isig = (self.termios.c_lflag & ISIG) != 0;
        let canon = (self.termios.c_lflag & ICANON) != 0;
        let echo = (self.termios.c_lflag & ECHO) != 0;

        if isig && byte == self.termios.c_cc[VINTR] {
            crate::console::write_user_bytes(b"^C\r\n");
            self.line_len = 0;
            self.queue.clear();
            
            if let Some(pgid) = self.foreground_pgrp {
                crate::task::process::send_signal_pgrp(pgid, crate::task::signal::SIGINT);
            }
            return;
        }

        if !canon {
            // Raw mode: immediately enqueue byte
            self.queue.push_back(byte);
            return;
        }

        // Canonical mode line editing
        if byte == 0x08 || byte == 0x7f {
            // Backspace
            if self.line_len > 0 {
                self.line_len -= 1;
                if echo {
                    crate::console::write_user_bytes(b"\x08 \x08");
                }
            }
            return;
        }

        if self.line_len < self.line_buf.len() - 1 {
            self.line_buf[self.line_len] = byte;
            self.line_len += 1;

            if echo {
                if byte == b'\r' || byte == b'\n' {
                    crate::console::write_user_bytes(b"\r\n");
                } else {
                    crate::console::write_user_bytes(&[byte]);
                }
            }

            if byte == b'\n' || byte == b'\r' {
                // Line complete: flush to queue
                for i in 0..self.line_len {
                    let b = if self.line_buf[i] == b'\r' {
                        b'\n'
                    } else {
                        self.line_buf[i]
                    };
                    self.queue.push_back(b);
                }
                self.line_len = 0;
            }
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut read = 0;
        while read < buf.len() {
            if let Some(b) = self.queue.pop_front() {
                buf[read] = b;
                read += 1;
            } else {
                break;
            }
        }
        read
    }

    pub fn write(&mut self, buf: &[u8]) -> usize {
        crate::console::write_user_bytes(buf);
        buf.len()
    }
}

pub static GLOBAL_TTY: InterruptMutex<Tty> = InterruptMutex::new(Tty::new());

pub use vfs::create_tty_file;

pub fn push_byte(byte: u8) {
    GLOBAL_TTY.lock().push_byte(byte);
    crate::arch::input::push_key(byte as char);
}
