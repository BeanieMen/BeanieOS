// IDT vectors.
pub const KEYBOARD_VECTOR: u8 = 33;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

// Local APIC MMIO base (default, overridden via MADT if needed).
pub const LAPIC_BASE: usize = 0xFEE0_0000;
pub const LAPIC_SVR: usize = LAPIC_BASE + 0xF0;
pub const LAPIC_ID_REG: usize = LAPIC_BASE + 0x20;
pub const LAPIC_EOI_REG: usize = LAPIC_BASE + 0xB0;

// SVR bits: enable APIC + spurious vector.
pub const LAPIC_SVR_ENABLE: u32 = 0x100;

// PS/2 keyboard data port.
pub const PS2_DATA_PORT: u16 = 0x60;

// Legacy 8259 PIC mask ports (used only to disable it).
pub const LEGACY_PIC0_MASK: u16 = 0x21;
pub const LEGACY_PIC1_MASK: u16 = 0xA1;
