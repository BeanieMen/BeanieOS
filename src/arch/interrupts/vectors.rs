use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::VirtAddr;
use x86_64::structures::idt::InterruptDescriptorTable;

use super::consts::LAPIC_TIMER_VECTOR;
use super::pic;

static TICKS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn timer_interrupt_entry();
}

pub(crate) fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
extern "C" fn timer_interrupt_rust() {
    TICKS.fetch_add(1, Ordering::Relaxed);

    unsafe {
        pic::eoi();
    }
}

pub(crate) fn register_vectors(idt: &mut InterruptDescriptorTable) {
    unsafe {
        idt[LAPIC_TIMER_VECTOR]
            .set_handler_addr(VirtAddr::from_ptr(timer_interrupt_entry as *const ()));
    }
}