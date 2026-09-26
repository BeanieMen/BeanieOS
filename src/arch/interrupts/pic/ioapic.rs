use super::super::consts::KEYBOARD_VECTOR;
use super::lapic;
use super::madt;

unsafe fn ioapic_write(base: usize, reg: u8, value: u32) {
    unsafe {
        core::ptr::write_volatile(base as *mut u32, reg as u32);
        core::ptr::write_volatile((base + 0x10) as *mut u32, value);
    }
}

pub(super) unsafe fn init() {
    let parsed = madt::get();
    let address = parsed.ioapic_address;
    let keyboard_gsi = parsed.keyboard_gsi;
    let gsi_base = parsed.ioapic_gsi_base;

    let index = keyboard_gsi - gsi_base;
    // calculates the low and high register offsets to setup I/O APIC redirection table entry corresponding to the keyboard GSI using its index
    // which is alo then calculated using keyboard gsi parsed from madt and gsi base of the I/O APIC.
    // demonic level of confusion caused by legacy stuff once again

    // explanation 
    // calculate low and high reg offsets for ioapic redirection table entry
    let low = 0x10 + index * 2;
    let high = low + 1;

    let lapic_id = unsafe { lapic::id() };

    unsafe {
        // lapic id is put into 24-31 bits of the high register 
        ioapic_write(address, high as u8, lapic_id << 24);
    }
    unsafe {
        // low register is set to wtv vector is gonna be sent
        ioapic_write(address, low as u8, KEYBOARD_VECTOR as u32);
    }
}
