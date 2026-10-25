pub mod ioapic;
pub mod lapic;
pub mod madt;

use super::consts::{
    LEGACY_PIC0_COMMAND, LEGACY_PIC0_MASK, LEGACY_PIC1_COMMAND, LEGACY_PIC1_MASK, PIC_EOI,
};

static mut CASCADE_OFFSET: (u8, u8) = (0, 0);

pub(crate) fn legacy_cascade() -> (u8, u8) {
    unsafe { CASCADE_OFFSET }
}

fn io_wait() {
    use x86_64::instructions::port::Port;
    unsafe {
        Port::<u8>::new(0x80).write(0);
    }
}

fn remap_legacy_pic() {
    use x86_64::instructions::port::Port;

    const MASTER_OFFSET: u8 = 0x20;
    const SLAVE_OFFSET: u8 = 0x28;

    unsafe {
        let mut command0 = Port::<u8>::new(LEGACY_PIC0_COMMAND);
        let mut command1 = Port::<u8>::new(LEGACY_PIC1_COMMAND);
        let mut data0 = Port::<u8>::new(LEGACY_PIC0_MASK);
        let mut data1 = Port::<u8>::new(LEGACY_PIC1_MASK);

        command0.write(0x11);
        io_wait();
        command1.write(0x11);
        io_wait();

        data0.write(MASTER_OFFSET);
        io_wait();
        data1.write(SLAVE_OFFSET);
        io_wait();

        data0.write(0x04);
        io_wait();
        data1.write(0x02);
        io_wait();

        data0.write(0x01);
        io_wait();
        data1.write(0x01);
        io_wait();

        CASCADE_OFFSET = (SLAVE_OFFSET + 2, MASTER_OFFSET + 2);
    }
}

fn mask_legacy_pic() {
    use x86_64::instructions::port::Port;

    const KEYBOARD_IRQ: u8 = 1;
    const CASCADE_IRQ: u8 = 2;
    const MOUSE_IRQ: u8 = 12 - 8;

    let master: u8 = !((1 << KEYBOARD_IRQ) | (1 << CASCADE_IRQ));
    let slave: u8 = !(1 << MOUSE_IRQ);

    unsafe {
        Port::new(LEGACY_PIC0_MASK).write(master);
        Port::new(LEGACY_PIC1_MASK).write(slave);
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
    mask_legacy_pic();
}

pub(crate) unsafe fn eoi() {
    use x86_64::instructions::port::Port;
    unsafe {
        lapic::send_eoi();
        ioapic::send_eoi();
        Port::<u8>::new(LEGACY_PIC1_COMMAND).write(PIC_EOI);
        Port::<u8>::new(LEGACY_PIC0_COMMAND).write(PIC_EOI);
    }
}
