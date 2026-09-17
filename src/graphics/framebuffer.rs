use core::fmt;
use spin::Mutex;

const FONT: &[u8] = include_bytes!("font.bin");
const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

impl Color {
    pub const fn to_rgb(self) -> u32 {
        match self {
            Color::Black => 0x000000,
            Color::Blue => 0x0000AA,
            Color::Green => 0x00AA00,
            Color::Cyan => 0x00AAAA,
            Color::Red => 0xAA0000,
            Color::Magenta => 0xAA00AA,
            Color::Brown => 0xAA5500,
            Color::LightGray => 0xAAAAAA,
            Color::DarkGray => 0x555555,
            Color::LightBlue => 0x5555FF,
            Color::LightGreen => 0x55FF55,
            Color::LightCyan => 0x55FFFF,
            Color::LightRed => 0xFF5555,
            Color::Pink => 0xFF55FF,
            Color::Yellow => 0xFFFF55,
            Color::White => 0xFFFFFF,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorCode {
    pub foreground: Color,
    pub background: Color,
}

impl ColorCode {
    pub const fn new(foreground: Color, background: Color) -> Self {
        Self {
            foreground,
            background,
        }
    }
}

pub struct Framebuffer {
    pub addr: *mut u8,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bpp: usize,
}

unsafe impl Send for Framebuffer {}

pub struct Writer {
    framebuffer: Option<Framebuffer>,
    cursor_col: usize,
    cursor_row: usize,
    color_code: ColorCode,
}

unsafe impl Send for Writer {}

impl Writer {
    pub const fn new() -> Self {
        Self {
            framebuffer: None,
            cursor_col: 0,
            cursor_row: 0,
            color_code: ColorCode::new(Color::Yellow, Color::Black),
        }
    }

    pub fn init(&mut self, addr: *mut u8, width: usize, height: usize, pitch: usize, bpp: usize) {
        self.framebuffer = Some(Framebuffer {
            addr,
            width,
            height,
            pitch,
            bpp,
        });
        self.cursor_col = 0;
        self.cursor_row = 0;

        // Clear the screen with the background color (black)
        unsafe {
            core::ptr::write_bytes(addr, 0, height * pitch);
        }
    }

    pub fn cols(&self) -> usize {
        match self.framebuffer {
            Some(ref fb) => fb.width / FONT_WIDTH,
            None => 80,
        }
    }

    pub fn rows(&self) -> usize {
        match self.framebuffer {
            Some(ref fb) => fb.height / FONT_HEIGHT,
            None => 25,
        }
    }

    pub fn put_pixel(&self, x: usize, y: usize, color: u32) {
        if let Some(ref fb) = self.framebuffer {
            if x < fb.width && y < fb.height {
                let pixel_offset = y * fb.pitch + x * (fb.bpp / 8);
                unsafe {
                    let ptr = fb.addr.add(pixel_offset);
                    if fb.bpp == 32 {
                        *(ptr as *mut u32) = color;
                    } else if fb.bpp == 24 {
                        *ptr = (color & 0xFF) as u8;
                        *ptr.add(1) = ((color >> 8) & 0xFF) as u8;
                        *ptr.add(2) = ((color >> 16) & 0xFF) as u8;
                    }
                }
            }
        }
    }

    fn draw_char(&mut self, col: usize, row: usize, c: u8, fg: u32, bg: u32) {
        if self.framebuffer.is_some() {
            let glyph_offset = (c as usize) * FONT_HEIGHT;
            let x_start = col * FONT_WIDTH;
            let y_start = row * FONT_HEIGHT;

            for y in 0..FONT_HEIGHT {
                let byte = FONT[glyph_offset + y];
                for x in 0..FONT_WIDTH {
                    let color = if (byte & (0x80 >> x)) != 0 { fg } else { bg };
                    self.put_pixel(x_start + x, y_start + y, color);
                }
            }
        }
    }

    fn scroll(&mut self) {
        if let Some(ref fb) = self.framebuffer {
            let bytes_per_line = fb.pitch;
            let shift = FONT_HEIGHT * bytes_per_line;
            let total_bytes = fb.height * bytes_per_line;

            if total_bytes > shift {
                unsafe {
                    core::ptr::copy(fb.addr.add(shift), fb.addr, total_bytes - shift);
                    let bottom_start = fb.addr.add(total_bytes - shift);
                    core::ptr::write_bytes(bottom_start, 0, shift);
                }
            }
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        let cols = self.cols();
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.cursor_col = 0,
            0x08 => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    let bg = self.color_code.background.to_rgb();
                    self.draw_char(self.cursor_col, self.cursor_row, b' ', bg, bg);
                }
            }
            byte => {
                if self.cursor_col >= cols {
                    self.new_line();
                }
                let fg = self.color_code.foreground.to_rgb();
                let bg = self.color_code.background.to_rgb();
                self.draw_char(self.cursor_col, self.cursor_row, byte, fg, bg);
                self.cursor_col += 1;
            }
        }
    }

    fn new_line(&mut self) {
        let rows = self.rows();
        self.cursor_col = 0;
        if self.cursor_row + 1 < rows {
            self.cursor_row += 1;
        } else {
            self.scroll();
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' | b'\r' | 0x08 => self.write_byte(byte),
                _ => self.write_byte(0xfe),
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

pub static WRITER: Mutex<Writer> = Mutex::new(Writer::new());

pub fn init_framebuffer(addr: u64, width: usize, height: usize, pitch: usize, bpp: usize) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        WRITER.lock().init(addr as *mut u8, width, height, pitch, bpp);
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::graphics::framebuffer::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::print!("{}\n", format_args!($($arg)*))
    };
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;
    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    })
}
