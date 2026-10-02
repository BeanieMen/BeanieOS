// IDT vectors.

pub const KEYBOARD_VECTOR: u8 = 33;
pub const LAPIC_TIMER_VECTOR: u8 = 32;

// Register offsets from the LAPIC base.

pub const LAPIC_SVR_OFFSET: usize = 0xF0;
pub const LAPIC_ID_OFFSET: usize = 0x20;
pub const LAPIC_EOI_OFFSET: usize = 0xB0;

pub const LAPIC_TIMER_OFFSET: usize = 0x320;
pub const LAPIC_TIMER_INITIAL_OFFSET: usize = 0x380;
pub const LAPIC_TIMER_DIVIDE_OFFSET: usize = 0x3E0;

// SVR bits: enable APIC 1<<8   |    spurious vector at (0xFF).
//        ...0001 0000 0000     | ...0000 1111 1111
//                      ...0001 1111 1111
//                           1   F   F

pub const LAPIC_SVR_ENABLE: u32 = 0x100;

pub const SPURIOUS_VECTOR: u8 = 0xFF;

// LAPIC timer.

pub const LAPIC_TIMER_PERIODIC: u32 = 1 << 17;

pub const LAPIC_TIMER_DIVIDE_16: u32 = 0b0011;

// PS/2 keyboard data port.

pub const PS2_DATA_PORT: u16 = 0x60;

// Legacy 8259 PIC mask ports (used only to disable it).

pub const LEGACY_PIC0_MASK: u16 = 0x21;
pub const LEGACY_PIC1_MASK: u16 = 0xA1;

const _: () = assert!(LAPIC_TIMER_VECTOR >= 32);
const _: () = assert!(KEYBOARD_VECTOR >= 32);
const _: () = assert!(LAPIC_TIMER_VECTOR != KEYBOARD_VECTOR);
const _: () = assert!(SPURIOUS_VECTOR != LAPIC_TIMER_VECTOR);
const _: () = assert!(SPURIOUS_VECTOR != KEYBOARD_VECTOR);
