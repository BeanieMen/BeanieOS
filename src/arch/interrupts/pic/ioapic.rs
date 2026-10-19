use super::super::consts::KEYBOARD_VECTOR;
use super::lapic;
use super::madt;

const IOAPIC_INDEX_OFFSET: u64 = 0x00;
const IOAPIC_WINDOW_OFFSET: u64 = 0x10;

const IOAPIC_ENTRY_STRIDE: u32 = 2;

const IOAPIC_ENTRY_LOW: u32 = 0x10;
const IOAPIC_ENTRY_HIGH: u32 = 0x11;

const IOAPIC_ENTRY_MASKED: u32 = 1 << 16;

const IOAPIC_ENTRY_LOGICAL: u32 = 1 << 11;

const IOAPIC_PHYSICAL_DEST: u32 = 0 << 11;

const IOAPIC_PHYSICAL_MODE: u32 = 0 << 10;

fn window(offset: u64) -> u64 {
    crate::paging::phys_to_direct(madt::get().ioapic_address as u64 + offset)
}

unsafe fn ioapic_write(reg: u32, value: u32) {
    unsafe {
        core::ptr::write_volatile(window(IOAPIC_INDEX_OFFSET) as *mut u32, reg);
        core::ptr::write_volatile(window(IOAPIC_WINDOW_OFFSET) as *mut u32, value);
    }
}

unsafe fn ioapic_read(reg: u32) -> u32 {
    unsafe {
        core::ptr::write_volatile(window(IOAPIC_INDEX_OFFSET) as *mut u32, reg);
        core::ptr::read_volatile(window(IOAPIC_WINDOW_OFFSET) as *const u32)
    }
}

pub(crate) fn route(gsi: u32, vector: u8) {
    let parsed = madt::get();

    let index = gsi.saturating_sub(parsed.ioapic_gsi_base);
    let low = IOAPIC_ENTRY_LOW + index * IOAPIC_ENTRY_STRIDE;

    let destination = unsafe { lapic::id() } << 24;

    unsafe {
        ioapic_write(low, IOAPIC_ENTRY_MASKED);

        ioapic_write(low + 1, destination);

        ioapic_write(
            low,
            vector as u32 | IOAPIC_ENTRY_LOGICAL | IOAPIC_PHYSICAL_DEST | IOAPIC_PHYSICAL_MODE,
        );
    }
}

pub(crate) fn mask(gsi: u32) {
    let parsed = madt::get();

    let index = gsi.saturating_sub(parsed.ioapic_gsi_base);
    let low = IOAPIC_ENTRY_LOW + index * IOAPIC_ENTRY_STRIDE;

    unsafe {
        ioapic_write(low, ioapic_read(low) | IOAPIC_ENTRY_MASKED);
    }
}

pub(super) unsafe fn send_eoi() {
    // no need to send EOI to IOAPIC, just LAPIC
}

pub(super) unsafe fn init() {
    let parsed = madt::get();

    unsafe {
        ioapic_write(0x00, 0x10);

        let max_entry = (ioapic_read(0x01) >> 16) & 0xFF;
        let redirection = ioapic_read(0x10);

        crate::kinfo!(
            "ioapic: id {}, {} entries, gsi base {}",
            (redirection >> 24) & 0xFF,
            max_entry + 1,
            parsed.ioapic_gsi_base
        );

        for entry in 0..=max_entry {
            ioapic_write(
                IOAPIC_ENTRY_LOW + entry * IOAPIC_ENTRY_STRIDE,
                IOAPIC_ENTRY_MASKED,
            );
        }
    }

    route(parsed.keyboard_gsi, KEYBOARD_VECTOR);

    crate::kinfo!("ioapic: keyboard gsi {}", parsed.keyboard_gsi);
}
