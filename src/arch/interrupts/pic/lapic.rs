use x86_64::instructions::port::Port;

use crate::arch::interrupts::consts::{
    LAPIC_EOI_OFFSET, LAPIC_ID_OFFSET, LAPIC_SVR_ENABLE, LAPIC_SVR_OFFSET,
    LAPIC_TIMER_CURRENT_OFFSET, LAPIC_TIMER_DIVIDE_16, LAPIC_TIMER_DIVIDE_OFFSET,
    LAPIC_TIMER_INITIAL_OFFSET, LAPIC_TIMER_OFFSET, LAPIC_TIMER_PERIODIC, LAPIC_TIMER_VECTOR,
    SPURIOUS_VECTOR, TICK_MS,
};

use super::madt;

const PIT_HZ: u64 = 1_193_182;

const PIT_WINDOW_TICKS: u64 = 59_659;

const PIT_RELOAD: u16 = 0;

const PIT_POLL_LIMIT: u32 = 10_000_000;

const NOMINAL_COUNTS_PER_MS: u32 = 62_500;

const PIT_COMMAND_PORT: u16 = 0x43;
const PIT_CHANNEL_PORT: u16 = 0x40;

fn lapic_base() -> u64 {
    madt::get().lapic_address as u64
}

fn read32(offset: usize) -> u32 {
    let addr = crate::paging::phys_to_direct(lapic_base() + offset as u64);

    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

fn write32(offset: usize, value: u32) {
    let addr = crate::paging::phys_to_direct(lapic_base() + offset as u64);

    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

pub(super) unsafe fn init() {
    let value = read32(LAPIC_SVR_OFFSET);

    write32(
        LAPIC_SVR_OFFSET,
        value | LAPIC_SVR_ENABLE | (SPURIOUS_VECTOR as u32) << 12,
    );

    crate::kinfo!("lapic id {}", id());
    crate::kinfo!("lapic base {:#x}", lapic_base());
}

pub(super) unsafe fn init_timer() {
    let counts = calibrate().unwrap_or_else(|| {
        crate::kwarn!("PIT did not advance, assuming {NOMINAL_COUNTS_PER_MS} counts/ms");
        NOMINAL_COUNTS_PER_MS
    });

    let initial = counts.saturating_mul(TICK_MS as u32);

    crate::kinfo!("lapic timer: {counts} counts/ms, {initial} counts per {TICK_MS}ms");

    write32(LAPIC_TIMER_DIVIDE_OFFSET, LAPIC_TIMER_DIVIDE_16);

    write32(
        LAPIC_TIMER_OFFSET,
        LAPIC_TIMER_PERIODIC | LAPIC_TIMER_VECTOR as u32,
    );

    write32(LAPIC_TIMER_INITIAL_OFFSET, initial);
}

fn calibrate() -> Option<u32> {
    write32(LAPIC_TIMER_DIVIDE_OFFSET, LAPIC_TIMER_DIVIDE_16);
    write32(LAPIC_TIMER_OFFSET, 0);

    let mut command = Port::<u8>::new(PIT_COMMAND_PORT);
    let mut channel = Port::<u8>::new(PIT_CHANNEL_PORT);

    unsafe {
        command.write(0x34u8);
        channel.write(PIT_RELOAD as u8);
        channel.write((PIT_RELOAD >> 8) as u8);
    }

    let mut previous = pit_count();
    let mut elapsed: u64 = 0;
    let mut polls: u32 = 0;

    write32(LAPIC_TIMER_INITIAL_OFFSET, u32::MAX);

    while elapsed < PIT_WINDOW_TICKS {
        core::hint::spin_loop();

        let now = pit_count();

        elapsed += previous.wrapping_sub(now) as u64;
        previous = now;

        polls += 1;

        if polls > PIT_POLL_LIMIT {
            crate::kwarn!("PIT never advanced: {polls} reads holding {now}");
            return None;
        }
    }

    let counts = u32::MAX.wrapping_sub(read32(LAPIC_TIMER_CURRENT_OFFSET)) as u64;

    let window_ns = elapsed * 1_000_000_000 / PIT_HZ;
    let per_ms = counts * 1_000_000 / window_ns;

    if per_ms == 0 || per_ms > u32::MAX as u64 {
        crate::kwarn!("implausible measurement: {counts} LAPIC counts in {window_ns} ns");
        return None;
    }

    Some(per_ms as u32)
}

fn pit_count() -> u16 {
    let mut command = Port::<u8>::new(PIT_COMMAND_PORT);
    let mut channel = Port::<u8>::new(PIT_CHANNEL_PORT);

    unsafe {
        command.write(0x00u8);

        let low = channel.read() as u16;
        let high = channel.read() as u16;

        (high << 8) | low
    }
}

pub(super) unsafe fn id() -> u32 {
    read32(LAPIC_ID_OFFSET) >> 24
}

pub(super) unsafe fn send_eoi() {
    write32(LAPIC_EOI_OFFSET, 0);
}