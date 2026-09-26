use core::fmt;
use spin::Mutex;

const FONT: &[u8] = include_bytes!("font.bin");
const FONT_WIDTH: u32 = 8;
const FONT_HEIGHT: u32 = 16;

#[allow(dead_code)]
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
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u8,
}

unsafe impl Send for Framebuffer {}

pub struct Writer {
    framebuffer: Option<Framebuffer>,
    cursor_col: u32,
    cursor_row: u32,
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

    pub fn init(&mut self, addr: *mut u8, width: u32, height: u32, pitch: u32, bpp: u8) {
        self.framebuffer = Some(Framebuffer {
            addr,
            width,
            height,
            pitch,
            bpp,
        });

        self.cursor_col = 0;
        self.cursor_row = 0;

        unsafe {
            core::ptr::write_bytes(addr, 0, (height * pitch) as usize);
        }
    }

    pub fn cols(&self) -> u32 {
        match self.framebuffer {
            Some(ref fb) => fb.width / FONT_WIDTH,
            None => 80,
        }
    }

    pub fn rows(&self) -> u32 {
        match self.framebuffer {
            Some(ref fb) => fb.height / FONT_HEIGHT,
            None => 25,
        }
    }

    pub fn put_pixel(&self, x: u32, y: u32, color: u32) {
        if let Some(ref fb) = self.framebuffer {
            if x >= fb.width || y >= fb.height {
                return;
            }

            let bytes_per_pixel = fb.bpp / 8;
            let pixel_offset = y * fb.pitch + x * bytes_per_pixel as u32;

            unsafe {
                let ptr = fb.addr.add(pixel_offset as usize);

                match fb.bpp {
                    8 => {
                        *ptr = color as u8;
                    }

                    16 => {
                        *(ptr as *mut u16) = color as u16;
                    }

                    24 => {
                        *ptr = (color & 0xFF) as u8;
                        *ptr.add(1) = ((color >> 8) & 0xFF) as u8;
                        *ptr.add(2) = ((color >> 16) & 0xFF) as u8;
                    }

                    32 => {
                        *(ptr as *mut u32) = color;
                    }

                    _ => {}
                }
            }
        }
    }

    /// Physical pixel row for a logical text row. The view is rigid:
    /// logical row r is always at pixel row r * FONT_HEIGHT, which is why
    /// scrolling has to move pixels (see `scroll`).
    fn row_y(&self, row: u32) -> u32 {
        row * FONT_HEIGHT
    }

    /// True when the 32bpp fast path can draw this glyph unclipped.
    fn glyph_fits(&self, col: u32, row: u32) -> bool {
        match self.framebuffer {
            Some(ref fb) if fb.bpp == 32 => {
                let x0 = col * FONT_WIDTH;
                let y0 = self.row_y(row);
                x0 + FONT_WIDTH <= fb.width && y0 + FONT_HEIGHT <= fb.height
            }
            _ => false,
        }
    }

    /// Row-at-a-time glyph blit. bpp, pitch and the row base pointer are
    /// resolved once instead of once per pixel.
    fn draw_char(&mut self, col: u32, row: u32, c: u8, fg: u32, bg: u32) {
        if !self.glyph_fits(col, row) {
            return self.draw_char_generic(col, row, c, fg, bg);
        }

        let Some(ref fb) = self.framebuffer else {
            return;
        };
        let x0 = col * FONT_WIDTH;
        let y0 = self.row_y(row);
        let bpp = (fb.bpp / 8) as usize;
        let glyph = &FONT[c as usize * FONT_HEIGHT as usize..][..FONT_HEIGHT as usize];
        let mut base = unsafe {
            fb.addr
                .add(y0 as usize * fb.pitch as usize + x0 as usize * bpp)
        };

        for &bits in glyph {
            let mut x = 0usize;
            while x < FONT_WIDTH as usize {
                if bits & (0x80 >> x) != 0 {
                    unsafe { (base.add(x * bpp) as *mut u32).write(fg) };
                    x += 1;
                } else {
                    // Walk the whole background run without re-testing the bit.
                    while x < FONT_WIDTH as usize && bits & (0x80 >> x) == 0 {
                        unsafe { (base.add(x * bpp) as *mut u32).write(bg) };
                        x += 1;
                    }
                }
            }
            base = unsafe { base.add(fb.pitch as usize) };
        }
    }

    /// Per-pixel fallback for non-32bpp depths and glyphs clipped at a
    /// screen edge.
    fn draw_char_generic(&mut self, col: u32, row: u32, c: u8, fg: u32, bg: u32) {
        if self.framebuffer.is_none() {
            return;
        }
        let glyph_offset = (c as usize) * FONT_HEIGHT as usize;
        let x_start = col * FONT_WIDTH;
        let y_start = self.row_y(row);

        for y in 0..FONT_HEIGHT {
            let byte = FONT[glyph_offset + y as usize];

            for x in 0..FONT_WIDTH {
                let color = if (byte & (0x80 >> x)) != 0 { fg } else { bg };

                self.put_pixel(x_start + x, y_start + y, color);
            }
        }
    }

    /// Scroll the text area up by one row.
    ///
    /// Only the `rows() * FONT_HEIGHT` pixel rows that actually hold text
    /// are moved, so a framebuffer height that is not a multiple of
    /// FONT_HEIGHT does not drag the last partial row along.
    fn scroll(&mut self) {
        let Some(ref fb) = self.framebuffer else {
            return;
        };
        let rows = self.rows();
        if rows <= 1 {
            return;
        }

        let text_bytes = rows as usize * FONT_HEIGHT as usize * fb.pitch as usize;
        let shift = FONT_HEIGHT as usize * fb.pitch as usize;
        if text_bytes <= shift {
            return;
        }

        unsafe {
            core::ptr::copy(fb.addr.add(shift), fb.addr, text_bytes - shift);
            core::ptr::write_bytes(fb.addr.add(text_bytes - shift), 0, shift);
        }
    }

    /// Zero one physical pixel row.
    fn clear_row(&self, y: u32) {
        let Some(ref fb) = self.framebuffer else {
            return;
        };
        if y >= fb.height {
            return;
        }
        unsafe {
            core::ptr::write_bytes(
                fb.addr.add(y as usize * fb.pitch as usize),
                0,
                fb.pitch as usize,
            )
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        let cols = self.cols();

        match byte {
            b'\n' => self.new_line(),

            b'\r' => {
                self.cursor_col = 0;
            }

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
                0x20..=0x7e | b'\n' | b'\r' | 0x08 => {
                    self.write_byte(byte);
                }

                _ => {
                    self.write_byte(0xfe);
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn clear(&self, _color: u32) {
        let Some(ref fb) = self.framebuffer else {
            return;
        };
        for y in 0..fb.height {
            self.clear_row(y);
        }
    }

    pub fn fill_rect(&self, x: u32, y: u32, width: u32, height: u32, color: u32) {
        let Some(ref fb) = self.framebuffer else {
            return;
        };
        if x >= fb.width || y >= fb.height {
            return;
        }

        let w = core::cmp::min(width, fb.width - x);
        let h = core::cmp::min(height, fb.height - y);
        let bpp = (fb.bpp / 8) as usize;

        if bpp == 4 {
            for dy in 0..h {
                let off = (y + dy) as usize * fb.pitch as usize + x as usize * bpp;
                for dx in 0..w {
                    unsafe { (fb.addr.add(off + dx as usize * 4) as *mut u32).write(color) }
                }
            }
        } else {
            for dy in 0..h {
                for dx in 0..w {
                    self.put_pixel(x + dx, y + dy, color)
                }
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

pub fn init_framebuffer(addr: u64, width: u32, height: u32, pitch: u32, bpp: u8) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        WRITER
            .lock()
            .init(addr as *mut u8, width, height, pitch, bpp);
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

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
    });
}
