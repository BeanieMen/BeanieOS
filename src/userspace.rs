use crate::graphics::framebuffer::WRITER;
use crate::task::scheduler;

static RGB_TICKS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// One leg of the colour cycle: green to red, then back, and so on.
const RAMP: u32 = 255;
const LEGS: u32 = 6;
const RAMP_TICKS: u32 = RAMP + 1;
const CYCLE: u32 = RAMP_TICKS * LEGS;

const STEP: u32 = 8;

const X: u32 = 500;
const Y: u32 = 500;
const WIDTH: u32 = 100;
const HEIGHT: u32 = 100;

pub extern "C" fn rgb_square_task() {
    loop {
        let tick = RGB_TICKS.fetch_add(STEP, core::sync::atomic::Ordering::Relaxed) % CYCLE;

        let ramp = tick % RAMP_TICKS;
        let leg = tick / RAMP_TICKS;

        let (r, g, b) = match leg {
            0 => (RAMP, ramp, 0),
            1 => (RAMP - ramp, RAMP, 0),
            2 => (0, RAMP, ramp),
            3 => (0, RAMP - ramp, RAMP),
            4 => (ramp, 0, RAMP),
            _ => (RAMP, 0, RAMP - ramp),
        };

        let color = (r << 16) | (g << 8) | b;

        WRITER.lock().fill_rect(X, Y, WIDTH, HEIGHT, color);

        scheduler::yield_now();
    }
}
