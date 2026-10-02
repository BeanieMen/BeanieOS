use alloc::boxed::Box;

use fatfs::FileSystem;
use spin::{Mutex, Once};

use crate::fs::BlockDevice;
use crate::graphics::framebuffer::WRITER;
use crate::shell::Shell;
use crate::task::scheduler;

pub static SHELL: Once<Mutex<Shell>> = Once::new();

pub fn install(fs: FileSystem<Box<dyn BlockDevice>>) {
    SHELL.call_once(|| Mutex::new(Shell::new(fs)));
}

pub extern "C" fn shell_task() {
    let shell = SHELL.get().expect("Shell not initialized");
    loop {
        if let Some(key) = crate::arch::input::pop_key() {
            shell.lock().shell_input(key);
        } else {
            scheduler::yield_now();
        }
    }
}

static RGB_TICKS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

pub extern "C" fn rgb_square_task() {
    loop {
        let tick = RGB_TICKS.fetch_add(8, core::sync::atomic::Ordering::Relaxed) % 1536;

        let (r, g, b) = match tick {
            0..=255 => (255, tick, 0),
            256..=511 => (511 - tick, 255, 0),
            512..=767 => (0, 255, tick - 512),
            768..=1023 => (0, 1023 - tick, 255),
            1024..=1279 => (tick - 1024, 0, 255),
            _ => (255, 0, 1535 - tick),
        };

        let color = (r << 16) | (g << 8) | b;

        WRITER.lock().fill_rect(500, 500, 100, 100, color);

        scheduler::yield_now();
    }
}
