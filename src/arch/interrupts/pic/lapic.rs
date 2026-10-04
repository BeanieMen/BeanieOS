use x86_64::instructions::port::Port;

use crate::arch::interrupts::consts::{
    LAPIC_EOI_OFFSET, LAPIC_ID_OFFSET, LAPIC_SVR_ENABLE, LAPIC_SVR_OFFSET,
    LAPIC_TIMER_CURRENT_OFFSET, LAPIC_TIMER_DIVIDE_16, LAPIC_TIMER_DIVIDE_OFFSET,
    LAPIC_TIMER_INITIAL_OFFSET, LAPIC_TIMER_OFFSET, LAPIC_TIMER_PERIODIC, LAPIC_TIMER_VECTOR,
    SPURIOUS_VECTOR, TICK_MS,
};

use super::madt;

/// 8254 crystal: the only rate here that can be taken rather than measured.
const PIT_HZ: u64 = 1_193_182;

/// 59659 PIT ticks is 50 ms at PIT_HZ.
const PIT_WINDOW_TICKS: u64 = 59_659;

/// Zero is how the PIT encodes 65536.
const PIT_RELOAD: u16 = 0;

/// Reads before giving up: a PIT that never advances must not stall the boot.
const PIT_POLL_LIMIT: u32 = 10_000_000;

/// Assumed when the PIT is silent: 1 GHz bus / 16, the emulator's rate.
const NOMINAL_COUNTS_PER_MS: u32 = 62_500;

fn lapic_base() -> usize {
    madt::get().lapic_address
}

fn lapic_svr_reg() -> usize {
    lapic_base() + LAPIC_SVR_OFFSET
}

fn lapic_id_reg() -> usize {
    lapic_base() + LAPIC_ID_OFFSET
}

fn lapic_eoi_reg() -> usize {
    lapic_base() + LAPIC_EOI_OFFSET
}

fn lapic_timer_reg() -> usize {
    lapic_base() + LAPIC_TIMER_OFFSET
}

fn lapic_timer_initial_reg() -> usize {
    lapic_base() + LAPIC_TIMER_INITIAL_OFFSET
}

fn lapic_timer_current_reg() -> usize {
    lapic_base() + LAPIC_TIMER_CURRENT_OFFSET
}

fn lapic_timer_divide_reg() -> usize {
    lapic_base() + LAPIC_TIMER_DIVIDE_OFFSET
}

pub(super) unsafe fn init() {
    let svr = lapic_svr_reg() as *mut u32;
    let value = unsafe { core::ptr::read_volatile(svr) };
    unsafe {
        core::ptr::write_volatile(svr, value | LAPIC_SVR_ENABLE | SPURIOUS_VECTOR as u32);
    }
}
/// One tick per `TICK_MS`, the period measured against the PIT. The LAPIC
/// bus clock is never published, so a tick is not a time unit until measured.
pub(super) unsafe fn init_timer() {
    let counts = calibrate().unwrap_or_else(|| {
        crate::kwarn!("PIT did not advance, assuming {NOMINAL_COUNTS_PER_MS} counts/ms");
        NOMINAL_COUNTS_PER_MS
    });

    let initial = counts.saturating_mul(TICK_MS as u32);

    crate::kinfo!("lapic timer: {counts} counts/ms, {initial} counts per {TICK_MS}ms");

    unsafe {
        core::ptr::write_volatile(lapic_timer_divide_reg() as *mut u32, LAPIC_TIMER_DIVIDE_16);

        core::ptr::write_volatile(
            lapic_timer_reg() as *mut u32,
            LAPIC_TIMER_PERIODIC | LAPIC_TIMER_VECTOR as u32,
        );

        core::ptr::write_volatile(lapic_timer_initial_reg() as *mut u32, initial);
    }
}

/// LAPIC counts per millisecond, measured against the PIT. Channel 0 free-runs
/// at PIT_HZ, so PIT ticks elapsed over the window time how far the LAPIC
/// drained. `None` means the PIT never advanced, not that it was wrong.
fn calibrate() -> Option<u32> {
    unsafe {
        // Not started: writing the initial count starts the one-shot, so the
        // window below sets when.
        core::ptr::write_volatile(lapic_timer_divide_reg() as *mut u32, LAPIC_TIMER_DIVIDE_16);
        core::ptr::write_volatile(lapic_timer_reg() as *mut u32, 0);
    }

    let mut command = Port::<u8>::new(0x43u16);
    let mut channel = Port::<u8>::new(0x40u16);

    unsafe {
        // Channel 0, mode 2, low byte then high byte, reload zero. The counter
        // wraps and keeps going, so both ends of the window can be sampled.
        command.write(0x34u8);
        channel.write(PIT_RELOAD as u8);
        channel.write((PIT_RELOAD >> 8) as u8);
    }

    let mut previous = pit_count();
    let mut elapsed: u64 = 0;
    let mut polls: u32 = 0;

    unsafe {
        // Full scale, so the window's drain is the whole clock.
        core::ptr::write_volatile(lapic_timer_initial_reg() as *mut u32, u32::MAX);
    }

    while elapsed < PIT_WINDOW_TICKS {
        core::hint::spin_loop();

        let now = pit_count();

        // Counts down and reloads: forwards closes the window, backwards
        // wraps. Both are one wrapping_sub.
        elapsed += previous.wrapping_sub(now) as u64;
        previous = now;

        polls += 1;

        if polls > PIT_POLL_LIMIT {
            crate::kwarn!("PIT never advanced: {polls} reads holding {now}");
            return None;
        }
    }

    let counts = unsafe { current_count() };
    let counts = u32::MAX.wrapping_sub(counts) as u64;

    let window_ns = elapsed * 1_000_000_000 / PIT_HZ;
    let per_ms = counts * 1_000_000 / window_ns;

    if per_ms == 0 || per_ms > u32::MAX as u64 {
        crate::kwarn!("implausible measurement: {counts} LAPIC counts in {window_ns} ns");
        return None;
    }

    Some(per_ms as u32)
}

/// Free-running down counter.
unsafe fn current_count() -> u32 {
    unsafe { core::ptr::read_volatile(lapic_timer_current_reg() as *const u32) }
}

/// Latch channel 0, then read low byte first.
fn pit_count() -> u16 {
    let mut command = Port::<u8>::new(0x43u16);
    let mut channel = Port::<u8>::new(0x40u16);

    unsafe {
        command.write(0x00u8);

        let low = channel.read() as u16;
        let high = channel.read() as u16;

        (high << 8) | low
    }
}

pub(super) unsafe fn id() -> u32 {
    unsafe { core::ptr::read_volatile(lapic_id_reg() as *const u32) >> 24 }
}

pub(super) unsafe fn send_eoi() {
    unsafe {
        core::ptr::write_volatile(lapic_eoi_reg() as *mut u32, 0);
    }
}
