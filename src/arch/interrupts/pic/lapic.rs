use crate::arch::interrupts::consts::{
    LAPIC_EOI_OFFSET, LAPIC_ID_OFFSET, LAPIC_SVR_ENABLE, LAPIC_SVR_OFFSET, LAPIC_TIMER_DIVIDE_16,
    LAPIC_TIMER_DIVIDE_OFFSET, LAPIC_TIMER_INITIAL_OFFSET, LAPIC_TIMER_OFFSET,
    LAPIC_TIMER_PERIODIC, LAPIC_TIMER_VECTOR, SPURIOUS_VECTOR,
};

use super::madt;

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
pub(super) unsafe fn init_timer(initial_count: u32) {
    unsafe {
        core::ptr::write_volatile(lapic_timer_divide_reg() as *mut u32, LAPIC_TIMER_DIVIDE_16);

        core::ptr::write_volatile(
            lapic_timer_reg() as *mut u32,
            LAPIC_TIMER_PERIODIC | LAPIC_TIMER_VECTOR as u32,
        );

        core::ptr::write_volatile(lapic_timer_initial_reg() as *mut u32, initial_count);
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
