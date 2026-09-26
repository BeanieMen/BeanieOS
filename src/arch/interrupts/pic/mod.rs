pub mod ioapic;
pub mod lapic;
pub mod madt;

use super::consts::{LEGACY_PIC0_MASK, LEGACY_PIC1_MASK};

fn disable_legacy_pic() {
    // 0x21/0xA1 are I/O *ports*, not MMIO addresses: they must be written
    // with `out`, not with a plain memory store. A store to address 0x21
    // would just corrupt low RAM (real-mode IVT) and leave the PIC live,
    // so its timer/keyboard IRQs would keep firing on vectors that have no
    // IDT entry -> #NP/#GP -> double fault.
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
        ioapic::init();
    }

    disable_legacy_pic();
}

pub unsafe fn eoi() {
    unsafe {
        lapic::send_eoi();
    }
}
