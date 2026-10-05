pub mod ioapic;
pub mod lapic;
pub mod madt;

use super::consts::{LEGACY_PIC0_MASK, LEGACY_PIC1_MASK};

fn disable_legacy_pic() {
    // 0x21/0xA1 are I/O ports, so `out`. A store there corrupts the real-mode
    // IVT and leaves the PIC firing IRQs with no IDT entry -> double fault.
    use x86_64::instructions::port::Port;
    unsafe {
        Port::new(LEGACY_PIC0_MASK).write(0xFFu8);
        Port::new(LEGACY_PIC1_MASK).write(0xFFu8);
    }
}

pub unsafe fn init(acpi_root_addr: usize) {
    unsafe {
        madt::init(acpi_root_addr);
    }
    unsafe {
        lapic::init();
    }
    unsafe {
        lapic::init_timer();
    }
    unsafe {
        ioapic::init();
    }

    disable_legacy_pic();
}

pub unsafe fn eoi() {
    unsafe {
        lapic::send_eoi();
    }
}
