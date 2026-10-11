pub mod ioapic;
pub mod lapic;
pub mod madt;

use super::consts::{LEGACY_PIC0_MASK, LEGACY_PIC1_MASK};

static mut cascade_offset: (u8, u8) = (0, 0);

pub(crate) fn legacy_cascade() -> (u8, u8) {
    unsafe { cascade_offset }
}

fn remap_legacy_pic() {
    use x86_64::instructions::port::Port;

    const MASTER_OFFSET: u8 = 0x20;
    const SLAVE_OFFSET: u8 = 0x28;

    unsafe {
        let mut mask0 = Port::<u8>::new(LEGACY_PIC0_MASK);
        let mut mask1 = Port::<u8>::new(LEGACY_PIC1_MASK);
        let mut command0 = Port::<u8>::new(0x20);
        let mut command1 = Port::<u8>::new(0xA0);
        let mut data0 = Port::<u8>::new(0x21);
        let mut data1 = Port::<u8>::new(0xA1);

        mask0.write(0xFF);
        mask1.write(0xFF);

        command0.write(0x11);
        data0.write(MASTER_OFFSET);
        data0.write(0x04);
        command0.write(0x01);
        data0.write(0x02);

        command1.write(0x11);
        data1.write(SLAVE_OFFSET);
        data1.write(0x02);
        command1.write(0x01);
        data1.write(0x02);

        cascade_offset = (SLAVE_OFFSET + 2, MASTER_OFFSET + 2);

        mask0.write(0);
        mask1.write(0);
    }
}

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

    unsafe {
        ioapic::init();
    }

    remap_legacy_pic();
    disable_legacy_pic();
}

pub(crate) unsafe fn eoi() {
    unsafe {
        lapic::send_eoi();
    }
}