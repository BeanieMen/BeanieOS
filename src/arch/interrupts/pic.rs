pub mod ioapic;
pub mod lapic;

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
    let madt = unsafe { ioapic::find_madt(acpi_root_addr) };
    let (ioapic, keyboard_gsi) = unsafe { ioapic::parse_madt(madt) };

    unsafe {
        lapic::init();
    }
    unsafe {
        ioapic::init(ioapic, keyboard_gsi);
    }

    disable_legacy_pic();
}

pub unsafe fn eoi() {
    unsafe {
        lapic::send_eoi();
    }
}
