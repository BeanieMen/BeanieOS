# Phase 5: High-Resolution Graphical Framebuffer & Console Output

## 🌟 High-Level Overview
In ancient DOS days, the computer's graphics card had a built-in "VGA Text Mode." You simply wrote ASCII bytes to memory address `0xB8000`, and the graphics card drew the letters on the screen for you.

On modern 64-bit computers (especially with UEFI firmware), **VGA Text Mode is dead**. The graphics card only gives you a flat grid of colored pixels called a **Linear Framebuffer**. 

If you want to print a letter 'A' on the screen, the graphics card will not help you. You have to calculate every single pixel coordinate and paint the individual colored dots yourself!

In this phase, we:
1. Grab the graphics card's framebuffer pointer from the Multiboot2 bootloader.
2. Build a 2D pixel-painting engine (`put_pixel` and `fill_rect`).
3. Embed an 8x16 bitmap font (`font.bin`) directly into the kernel binary.
4. Implement terminal scrolling and wire up our custom `print!` and `println!` macros.

---

## 📖 Layman's Glossary: Jargon Demystified

*   **Linear Framebuffer:**
    A giant continuous block of memory where each group of bytes represents the color of a single pixel on the physical monitor. Writing numbers into this memory instantly changes what is shown on screen.
*   **Resolution (Width & Height):**
    The number of horizontal and vertical pixels (e.g., 1024 pixels wide by 768 pixels tall).
*   **BPP (Bits Per Pixel):**
    How many bits are used to store one pixel's color:
    *   **32 BPP (True Color):** 4 bytes per pixel: Red (8 bits), Green (8 bits), Blue (8 bits), and 8 bits of padding/alpha.
*   **Pitch (or Stride):**
    The total number of *bytes* from the start of one horizontal line of pixels to the start of the next line.
    *   *Gotcha:* You might think `Pitch = Width * BytesPerPixel`. But graphics hardware often pads lines with invisible extra bytes to align them with memory bus boundaries (e.g., multiples of 64 or 128 bytes)! If you ignore Pitch, your screen will look like static diagonal television noise.
*   **Bitmap Font:**
    A black-and-white grid showing the shape of characters. Our font is **8 pixels wide and 16 pixels high**. Each character takes 16 bytes: each byte represents one row of 8 pixels, where a `1` bit means "draw letter color" and a `0` bit means "draw background color."
*   **Terminal Scrolling:**
    When you reach the bottom of the screen, everything must move up by one line (16 pixels) to make room for new text.

---

## 🗺️ What Files are Involved?
1. [src/graphics/framebuffer.rs](file:///home/aj/BeanieOS/src/graphics/framebuffer.rs) — The entire framebuffer driver, font renderer, scrolling engine, and writer.
2. [src/graphics/font.bin](file:///home/aj/BeanieOS/src/graphics/framebuffer.rs#L4) — The raw 8x16 binary font glyph table.
3. [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L149-L155) — Calls `init_framebuffer` using parameters from the bootloader tag.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Receiving the Screen Specifications
Located at lines 131–135 & 149–155 of [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L131-L155):

The bootloader fills out a `FramebufferTag` based on what our Phase 2 header requested:
```rust
let fb_tag = boot_info.framebuffer_tag().unwrap().unwrap();

graphics::framebuffer::init_framebuffer(
    fb_tag.address(), // e.g. 0xFD00_0000 in VRAM
    fb_tag.width(),   // 1024
    fb_tag.height(),  // 768
    fb_tag.pitch(),   // e.g. 4096 bytes per line
    fb_tag.bpp(),     // 32
);
```

---

### Step 2: The Core Pixel Painter (`put_pixel`)
Located at lines 128–163 of [src/graphics/framebuffer.rs](file:///home/aj/BeanieOS/src/graphics/framebuffer.rs#L128-L163):

How does an $(X, Y)$ coordinate turn into a pointer in RAM?
$$\text{Pixel Offset} = (Y \times \text{Pitch}) + \left(X \times \frac{\text{BPP}}{8}\right)$$

```rust
pub fn put_pixel(&self, x: u32, y: u32, color: u32) {
    if let Some(ref fb) = self.framebuffer {
        if x >= fb.width || y >= fb.height {
            return; // Safety guard: do not draw outside the monitor!
        }

        let bytes_per_pixel = fb.bpp / 8;
        let pixel_offset = y * fb.pitch + x * bytes_per_pixel as u32;

        unsafe {
            let ptr = fb.addr.add(pixel_offset as usize);
            if fb.bpp == 32 {
                *(ptr as *mut u32) = color; // Fast 32-bit single write
            }
        }
    }
}
```

---

### Step 3: Drawing Characters from the Bitmap Font
Located at lines 183–215 of [src/graphics/framebuffer.rs](file:///home/aj/BeanieOS/src/graphics/framebuffer.rs#L183-L215):

To draw an ASCII character (like `'B'` = ASCII 66):
1.  Look up the 16 bytes for character 66 in `FONT`.
2.  For each of the 16 rows:
    *   Inspect each of the 8 bits.
    *   If bit is `1`, paint pixel with Foreground color (Yellow).
    *   If bit is `0`, paint pixel with Background color (Black).

```rust
fn draw_char(&mut self, col: u32, row: u32, c: u8, fg: u32, bg: u32) {
    let x0 = col * FONT_WIDTH;   // col * 8
    let y0 = row * FONT_HEIGHT;  // row * 16
    let glyph = &FONT[c as usize * 16..][..16];
    
    let mut base = fb.addr.add(y0 * fb.pitch + x0 * 4);

    for &bits in glyph {
        for x in 0..8 {
            let color = if (bits & (0x80 >> x)) != 0 { fg } else { bg };
            (base.add(x * 4) as *mut u32).write(color);
        }
        base = base.add(fb.pitch); // Move down to next scanline
    }
}
```

---

### Step 4: Hardware Scrolling Engine
Located at lines 240–260 of [src/graphics/framebuffer.rs](file:///home/aj/BeanieOS/src/graphics/framebuffer.rs#L240-L260):

When text reaches the bottom row of the screen, `scroll()` is called:
```rust
fn scroll(&mut self) {
    let rows = self.rows();
    let text_bytes = rows as usize * FONT_HEIGHT as usize * fb.pitch as usize;
    let shift = FONT_HEIGHT as usize * fb.pitch as usize; // Height of 1 text line

    unsafe {
        // Copy pixel rows upwards by 1 line height
        core::ptr::copy(fb.addr.add(shift), fb.addr, text_bytes - shift);
        // Blank out the newly vacated bottom line with black pixels
        core::ptr::write_bytes(fb.addr.add(text_bytes - shift), 0, shift);
    }
}
```

---

### Step 5: Connecting to `println!`
Located at lines 398–424 of [src/graphics/framebuffer.rs](file:///home/aj/BeanieOS/src/graphics/framebuffer.rs#L398-L424):

The `Writer` implements Rust's `core::fmt::Write` trait. We wrap it in a thread-safe `spin::Mutex`:
```rust
pub static WRITER: Mutex<Writer> = Mutex::new(Writer::new());

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        $crate::print!("{}\n", format_args!($($arg)*))
    };
}
```
*   **Notice the critical safety guard:**
    In `_print`, the lock is acquired inside `interrupts::without_interrupts(|| { ... })`.
    Why? If an interrupt occurs *while* `println!` holds the `WRITER` lock, and the interrupt handler also tries to call `println!`, the system would deadlock forever trying to acquire a lock it already holds!

---

## 🎯 Summary Checklist
By the end of Phase 5, our operating system has:
1. Connected directly to the graphics card's linear framebuffer in video memory.
2. Built an $(X, Y)$ coordinate mapper respecting screen pitch and bit depth.
3. Created an 8x16 font rendering engine that paints text glyph by glyph.
4. Implemented full screen scrolling and thread-safe `print!` / `println!` macros.
