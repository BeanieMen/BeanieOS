pub(crate) const KEYBOARD_VECTOR: u8 = 33;
pub(crate) const LAPIC_TIMER_VECTOR: u8 = 32;
pub(crate) const AHCI_VECTOR: u8 = 34;

pub(crate) const LAPIC_SVR_OFFSET: usize = 0xF0;
pub(crate) const LAPIC_ID_OFFSET: usize = 0x20;
pub(crate) const LAPIC_EOI_OFFSET: usize = 0xB0;

pub(crate) const LAPIC_TIMER_OFFSET: usize = 0x320;
pub(crate) const LAPIC_TIMER_INITIAL_OFFSET: usize = 0x380;
pub(crate) const LAPIC_TIMER_CURRENT_OFFSET: usize = 0x390;
pub(crate) const LAPIC_TIMER_DIVIDE_OFFSET: usize = 0x3E0;

pub(crate) const TICK_MS: u64 = 10;

pub(crate) const LAPIC_SVR_ENABLE: u32 = 0x100;

pub(crate) const SPURIOUS_VECTOR: u8 = 0xFF;

pub(crate) const LAPIC_TIMER_PERIODIC: u32 = 1 << 17;

pub(crate) const LAPIC_TIMER_DIVIDE_16: u32 = 0b0011;

pub(crate) const PS2_DATA_PORT: u16 = 0x60;
pub(crate) const PS2_STATUS_PORT: u16 = 0x64;

pub(crate) const LEGACY_PIC0_MASK: u16 = 0x21;
pub(crate) const LEGACY_PIC1_MASK: u16 = 0xA1;
pub(crate) const LEGACY_PIC0_COMMAND: u16 = 0x20;

const _: () = assert!(LAPIC_TIMER_VECTOR >= 32);
const _: () = assert!(KEYBOARD_VECTOR >= 32);
const _: () = assert!(AHCI_VECTOR >= 32);
const _: () = assert!(LAPIC_TIMER_VECTOR != KEYBOARD_VECTOR);
const _: () = assert!(LAPIC_TIMER_VECTOR != AHCI_VECTOR);
const _: () = assert!(KEYBOARD_VECTOR != AHCI_VECTOR);
const _: () = assert!(SPURIOUS_VECTOR != LAPIC_TIMER_VECTOR);
const _: () = assert!(SPURIOUS_VECTOR != KEYBOARD_VECTOR);
const _: () = assert!(SPURIOUS_VECTOR != AHCI_VECTOR);