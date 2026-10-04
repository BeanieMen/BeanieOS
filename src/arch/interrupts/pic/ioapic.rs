use super::super::consts::KEYBOARD_VECTOR;
use super::lapic;
use super::madt;

unsafe fn ioapic_write(base: usize, reg: u8, value: u32) {
    unsafe {
        core::ptr::write_volatile(base as *mut u32, reg as u32);
        core::ptr::write_volatile((base + 0x10) as *mut u32, value);
    }
}

/// Sends `gsi` to `vector`. Entry registers sit at 0x10 + 2 * index, index
/// being the GSI above the I/O APIC base: low holds the vector, high holds the
/// destination LAPIC id in bits 24-31.
pub fn route(gsi: u32, vector: u8) {
    let parsed = madt::get();
    let address = parsed.ioapic_address;

    let index = gsi.saturating_sub(parsed.ioapic_gsi_base);
    let low = 0x10 + index * 2;

    let lapic_id = unsafe { lapic::id() };

    unsafe {
        ioapic_write(address, (low + 1) as u8, lapic_id << 24);
    }
    unsafe {
        ioapic_write(address, low as u8, vector as u32);
    }
}

pub(super) unsafe fn init() {
    let keyboard_gsi = madt::get().keyboard_gsi;

    route(keyboard_gsi, KEYBOARD_VECTOR);
}
