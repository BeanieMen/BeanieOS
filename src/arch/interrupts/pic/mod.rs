pub mod lapic;
pub mod madt;

use super::consts::{LEGACY_PIC0_MASK, LEGACY_PIC1_MASK};

fn disable_legacy_pic() {
    use x86_64::instructions::port::Port;

    unsafe {
        Port::new(LEGACY_PIC0_MASK).write(0xFFu8);
        Port::new(LEGACY_PIC1_MASK).write(0xFFu8);
    }
}

pub(crate) unsafe fn init(acpi_root_addr: usize) {
    unsafe {
        madt::init(acpi_root_addr);
    }

    crate::kinfo!(
        "MADT: lapic {:#x}, ioapic {:#x}, ioapic gsi base {}",
        madt::get().lapic_address,
        madt::get().ioapic_address,
        madt::get().ioapic_gsi_base
    );

    unsafe {
        lapic::init();
    }

    unsafe {
        lapic::init_timer();
    }

    disable_legacy_pic();
}

pub(crate) unsafe fn eoi() {
    unsafe {
        lapic::send_eoi();
    }
}