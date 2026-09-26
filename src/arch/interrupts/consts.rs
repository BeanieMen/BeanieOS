use super::pic::madt;

// IDT vectors.
pub const KEYBOARD_VECTOR: u8 = 33;

// Register offsets from the LAPIC base.
pub const LAPIC_SVR_OFFSET: usize = 0xF0;
pub const LAPIC_ID_OFFSET: usize = 0x20;
pub const LAPIC_EOI_OFFSET: usize = 0xB0;

// SVR bits: enable APIC 1<<8   |    spurious vector at (0xFF).
//        ...0001 0000 0000     | ...0000 1111 1111
//                      ...0001 1111 1111
//                           1   F   F

pub const LAPIC_SVR_ENABLE: u32 = 0x100;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

// PS/2 keyboard data port.
pub const PS2_DATA_PORT: u16 = 0x60;

// Legacy 8259 PIC mask ports (used only to disable it).
pub const LEGACY_PIC0_MASK: u16 = 0x21;
pub const LEGACY_PIC1_MASK: u16 = 0xA1;

pub fn lapic_base() -> usize {
    madt::get().lapic_address
}

pub fn lapic_svr_reg() -> usize {
    lapic_base() + LAPIC_SVR_OFFSET
}

pub fn lapic_id_reg() -> usize {
    lapic_base() + LAPIC_ID_OFFSET
}

pub fn lapic_eoi_reg() -> usize {
    lapic_base() + LAPIC_EOI_OFFSET
}
