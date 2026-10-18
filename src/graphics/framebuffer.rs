use core::fmt;

use crate::arch::lock::InterruptMutex;
use crate::paging::phys_to_direct;

const FONT: &[u8] = include_bytes!("font.bin");
const FONT_WIDTH: u32 = 8;
const FONT_HEIGHT: u32 = 16;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Color {
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
    const fn to_rgb(self) -> u32 {
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
enum AnsiState {
    Normal,
    Escape,
    Csi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ColorCode {
    pub foreground: Color,
    pub background: Color,
}

impl ColorCode {
    pub(crate) const fn new(foreground: Color, background: Color) -> Self {
        Self {
            foreground,
            background,
        }
    }
}

pub(crate) struct Framebuffer {
    pub addr: *mut u8,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u8,
}

unsafe impl Send for Framebuffer {}

pub(crate) struct Writer {
    pub framebuffer: Option<Framebuffer>,
    cursor_col: u32,
    cursor_row: u32,
    color_code: ColorCode,
    ansi_state: AnsiState,
    ansi_params: [u32; 8],
    ansi_param_idx: usize,
}

unsafe impl Send for Writer {}

impl Writer {
    pub(crate) const fn new() -> Self {
        Self {
            framebuffer: None,
            cursor_col: 0,
            cursor_row: 0,
            color_code: ColorCode::new(Color::Yellow, Color::Black),
            ansi_state: AnsiState::Normal,
            ansi_params: [0; 8],
            ansi_param_idx: 0,
        }
    }

    pub(crate) fn init(&mut self, addr: *mut u8, width: u32, height: u32, pitch: u32, bpp: u8) {
        self.framebuffer = Some(Framebuffer {
            addr,
            width,
            height,
            pitch,
            bpp,
        });

        self.cursor_col = 0;
        self.cursor_row = 0;
        self.ansi_state = AnsiState::Normal;
        unsafe {
            core::ptr::write_bytes(addr, 0, (height * pitch) as usize);
        }
    }

    pub(crate) fn cols(&self) -> u32 {
        match self.framebuffer {
            Some(ref fb) => fb.width / FONT_WIDTH,
            None => 80,
        }
    }

    pub(crate) fn rows(&self) -> u32 {
        match self.framebuffer {
            Some(ref fb) => fb.height / FONT_HEIGHT,
            None => 25,
        }
    }

    fn put_pixel(&self, x: u32, y: u32, color: u32) {
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

    fn row_y(&self, row: u32) -> u32 {
        row * FONT_HEIGHT
    }

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
                    while x < FONT_WIDTH as usize && bits & (0x80 >> x) == 0 {
                        unsafe { (base.add(x * bpp) as *mut u32).write(bg) };
                        x += 1;
                    }
                }
            }
            base = unsafe { base.add(fb.pitch as usize) };
        }
    }

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

    pub(crate) fn clear_screen(&mut self) {
        let Some(ref fb) = self.framebuffer else {
            return;
        };

        let bytes = fb.height as usize * fb.pitch as usize;
        unsafe {
            core::ptr::write_bytes(fb.addr, 0, bytes);
        }
    }

    pub(crate) fn clear_line(&mut self) {
        let cols = self.cols();
        let bg = self.color_code.background.to_rgb();
        for col in self.cursor_col..cols {
            self.draw_char(col, self.cursor_row, b' ', bg, bg);
        }
    }

    fn handle_sgr(&mut self) {
        let count = (self.ansi_param_idx + 1).min(8);
        for i in 0..count {
            match self.ansi_params[i] {
                0 => self.color_code = ColorCode::new(Color::White, Color::Black),
                30 => self.color_code.foreground = Color::Black,
                31 => self.color_code.foreground = Color::Red,
                32 => self.color_code.foreground = Color::Green,
                33 => self.color_code.foreground = Color::Yellow,
                34 => self.color_code.foreground = Color::Blue,
                35 => self.color_code.foreground = Color::Magenta,
                36 => self.color_code.foreground = Color::Cyan,
                37 | 39 => self.color_code.foreground = Color::White,
                40 | 49 => self.color_code.background = Color::Black,
                41 => self.color_code.background = Color::Red,
                42 => self.color_code.background = Color::Green,
                43 => self.color_code.background = Color::Yellow,
                44 => self.color_code.background = Color::Blue,
                45 => self.color_code.background = Color::Magenta,
                46 => self.color_code.background = Color::Cyan,
                47 => self.color_code.background = Color::White,
                90 => self.color_code.foreground = Color::DarkGray,
                91 => self.color_code.foreground = Color::LightRed,
                92 => self.color_code.foreground = Color::LightGreen,
                93 => self.color_code.foreground = Color::Yellow,
                94 => self.color_code.foreground = Color::LightBlue,
                95 => self.color_code.foreground = Color::Pink,
                96 => self.color_code.foreground = Color::LightCyan,
                97 => self.color_code.foreground = Color::White,
                _ => {}
            }
        }
    }

    pub(crate) fn write_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiState::Normal => {
                if byte == 0x1b {
                    self.ansi_state = AnsiState::Escape;
                    return;
                }
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
                    b'\t' => {
                        let next_col = (self.cursor_col + 8) & !7;
                        let cols = self.cols();
                        self.cursor_col = next_col.min(cols.saturating_sub(1));
                    }
                    b => {
                        let ch = if (0x20..=0x7e).contains(&b) { b } else { 0xfe };
                        let cols = self.cols();
                        if self.cursor_col >= cols {
                            self.new_line();
                        }
                        let fg = self.color_code.foreground.to_rgb();
                        let bg = self.color_code.background.to_rgb();
                        self.draw_char(self.cursor_col, self.cursor_row, ch, fg, bg);
                        self.cursor_col += 1;
                    }
                }
            }
            AnsiState::Escape => match byte {
                b'[' => {
                    self.ansi_state = AnsiState::Csi;
                    self.ansi_params = [0; 8];
                    self.ansi_param_idx = 0;
                }
                0x1b => {}
                _ => {
                    self.ansi_state = AnsiState::Normal;
                }
            },
            AnsiState::Csi => match byte {
                b'\n' | b'\r' => {
                    self.ansi_state = AnsiState::Normal;
                    if byte == b'\n' {
                        self.new_line();
                    } else {
                        self.cursor_col = 0;
                    }
                }
                b'0'..=b'9' => {
                    let idx = self.ansi_param_idx.min(7);
                    self.ansi_params[idx] = self.ansi_params[idx] * 10 + (byte - b'0') as u32;
                }
                b';' => {
                    if self.ansi_param_idx < 7 {
                        self.ansi_param_idx += 1;
                    }
                }
                b'?' => {}
                b'H' | b'f' => {
                    let row = if self.ansi_params[0] > 0 {
                        self.ansi_params[0] - 1
                    } else {
                        0
                    };
                    let col = if self.ansi_param_idx > 0 && self.ansi_params[1] > 0 {
                        self.ansi_params[1] - 1
                    } else {
                        0
                    };
                    let max_rows = self.rows();
                    let max_cols = self.cols();
                    self.cursor_row = row.min(max_rows.saturating_sub(1));
                    self.cursor_col = col.min(max_cols.saturating_sub(1));
                    self.ansi_state = AnsiState::Normal;
                }
                b'J' => {
                    if self.ansi_params[0] == 2 || self.ansi_params[0] == 0 {
                        self.clear_screen();
                        self.cursor_row = 0;
                        self.cursor_col = 0;
                    }
                    self.ansi_state = AnsiState::Normal;
                }
                b'K' => {
                    self.clear_line();
                    self.ansi_state = AnsiState::Normal;
                }
                b'A' => {
                    let count = self.ansi_params[0].max(1);
                    self.cursor_row = self.cursor_row.saturating_sub(count);
                    self.ansi_state = AnsiState::Normal;
                }
                b'B' => {
                    let count = self.ansi_params[0].max(1);
                    let max_rows = self.rows();
                    self.cursor_row = (self.cursor_row + count).min(max_rows.saturating_sub(1));
                    self.ansi_state = AnsiState::Normal;
                }
                b'C' => {
                    let count = self.ansi_params[0].max(1);
                    let max_cols = self.cols();
                    self.cursor_col = (self.cursor_col + count).min(max_cols.saturating_sub(1));
                    self.ansi_state = AnsiState::Normal;
                }
                b'D' => {
                    let count = self.ansi_params[0].max(1);
                    self.cursor_col = self.cursor_col.saturating_sub(count);
                    self.ansi_state = AnsiState::Normal;
                }
                b'm' => {
                    self.handle_sgr();
                    self.ansi_state = AnsiState::Normal;
                }
                _ => {
                    self.ansi_state = AnsiState::Normal;
                }
            },
        }
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_byte(byte);
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

    fn write_string(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }

    pub(crate) fn fill_rect(&self, x: u32, y: u32, width: u32, height: u32, color: u32) {
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

pub(crate) static WRITER: InterruptMutex<Writer> = InterruptMutex::new(Writer::new());

pub(crate) fn framebuffer_range() -> Option<(u64, u64)> {
    let writer = WRITER.lock();
    let fb = writer.framebuffer.as_ref()?;

    Some((
        fb.addr as u64,
        fb.addr as u64 + (fb.height * fb.pitch) as u64,
    ))
}

pub(crate) fn init_framebuffer(phys: u64, width: u32, height: u32, pitch: u32, bpp: u8) {
    let virt = phys_to_direct(phys);

    x86_64::instructions::interrupts::without_interrupts(|| {
        WRITER
            .lock()
            .init(virt as *mut u8, width, height, pitch, bpp);
    });
}
