use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use x86_64::VirtAddr;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptDescriptorTable;

use super::consts::{
    AHCI_VECTOR, KEYBOARD_VECTOR, LAPIC_TIMER_VECTOR, PS2_DATA_PORT, SPURIOUS_VECTOR,
};
use super::pic;

static TICKS: AtomicU64 = AtomicU64::new(0);
static KEYS: AtomicU64 = AtomicU64::new(0);
static SHIFT_ACTIVE: AtomicBool = AtomicBool::new(false);
static CTRL_ACTIVE: AtomicBool = AtomicBool::new(false);
static EXT_PREFIX: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn timer_interrupt_entry();
}

pub(crate) fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub(crate) fn keys() -> u64 {
    KEYS.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
extern "C" fn timer_interrupt_rust() {
    let tick = TICKS.fetch_add(1, Ordering::Relaxed);
    // Poll serial input every 8 ticks (8ms) rather than every tick to eliminate 87.5% of port 0x3fd VM-exits
    if tick.is_multiple_of(8) {
        unsafe {
            let mut lsr = Port::<u8>::new(0x3fd);
            while lsr.read() & 1 != 0 {
                let mut data = Port::<u8>::new(0x3f8);
                let byte = data.read();
                let ch = match byte {
                    b'\r' => b'\n',
                    0x7f | 0x08 => 0x08,
                    b => b,
                };
                crate::tty::push_byte(ch);
                KEYS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    crate::task::scheduler::account_tick();
    unsafe {
        pic::eoi();
    }
}

extern "x86-interrupt" fn spurious_interrupt_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    let mut port = Port::<u8>::new(PS2_DATA_PORT);
    let scancode: u8 = unsafe { port.read() };

    handle_keyboard_scancode(scancode);

    unsafe {
        pic::eoi();
    }
}

pub(crate) fn register_vectors(idt: &mut InterruptDescriptorTable) {
    idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_interrupt_handler);
    idt[SPURIOUS_VECTOR].set_handler_fn(spurious_interrupt_handler);
    idt[AHCI_VECTOR].set_handler_fn(crate::storage::ahci::on_interrupt);

    unsafe {
        idt[LAPIC_TIMER_VECTOR]
            .set_handler_addr(VirtAddr::from_ptr(timer_interrupt_entry as *const ()));
    }
}

const SCANCODE_MAP: [u8; 128] = {
    let mut map = [0; 128];

    map[0x02] = b'1';
    map[0x03] = b'2';
    map[0x04] = b'3';
    map[0x05] = b'4';
    map[0x06] = b'5';
    map[0x07] = b'6';
    map[0x08] = b'7';
    map[0x09] = b'8';
    map[0x0A] = b'9';
    map[0x0B] = b'0';
    map[0x0C] = b'-';
    map[0x0D] = b'=';
    map[0x0E] = 8;
    map[0x0F] = 9;

    map[0x10] = b'q';
    map[0x11] = b'w';
    map[0x12] = b'e';
    map[0x13] = b'r';
    map[0x14] = b't';
    map[0x15] = b'y';
    map[0x16] = b'u';
    map[0x17] = b'i';
    map[0x18] = b'o';
    map[0x19] = b'p';
    map[0x1A] = b'[';
    map[0x1B] = b']';
    map[0x1C] = 10;

    map[0x1E] = b'a';
    map[0x1F] = b's';
    map[0x20] = b'd';
    map[0x21] = b'f';
    map[0x22] = b'g';
    map[0x23] = b'h';
    map[0x24] = b'j';
    map[0x25] = b'k';
    map[0x26] = b'l';
    map[0x27] = b';';
    map[0x28] = b'\'';
    map[0x29] = b'`';

    map[0x2B] = b'\\';
    map[0x2C] = b'z';
    map[0x2D] = b'x';
    map[0x2E] = b'c';
    map[0x2F] = b'v';
    map[0x30] = b'b';
    map[0x31] = b'n';
    map[0x32] = b'm';
    map[0x33] = b',';
    map[0x34] = b'.';
    map[0x35] = b'/';

    map[0x39] = b' ';

    map
};

const SCANCODE_SHIFT_MAP: [u8; 128] = {
    let mut map = [0; 128];

    map[0x02] = b'!';
    map[0x03] = b'@';
    map[0x04] = b'#';
    map[0x05] = b'$';
    map[0x06] = b'%';
    map[0x07] = b'^';
    map[0x08] = b'&';
    map[0x09] = b'*';
    map[0x0A] = b'(';
    map[0x0B] = b')';
    map[0x0C] = b'_';
    map[0x0D] = b'+';
    map[0x0E] = 8;
    map[0x0F] = 9;

    map[0x10] = b'Q';
    map[0x11] = b'W';
    map[0x12] = b'E';
    map[0x13] = b'R';
    map[0x14] = b'T';
    map[0x15] = b'Y';
    map[0x16] = b'U';
    map[0x17] = b'I';
    map[0x18] = b'O';
    map[0x19] = b'P';
    map[0x1A] = b'{';
    map[0x1B] = b'}';
    map[0x1C] = 10;

    map[0x1E] = b'A';
    map[0x1F] = b'S';
    map[0x20] = b'D';
    map[0x21] = b'F';
    map[0x22] = b'G';
    map[0x23] = b'H';
    map[0x24] = b'J';
    map[0x25] = b'K';
    map[0x26] = b'L';
    map[0x27] = b':';
    map[0x28] = b'"';
    map[0x29] = b'~';

    map[0x2B] = b'|';
    map[0x2C] = b'Z';
    map[0x2D] = b'X';
    map[0x2E] = b'C';
    map[0x2F] = b'V';
    map[0x30] = b'B';
    map[0x31] = b'N';
    map[0x32] = b'M';
    map[0x33] = b'<';
    map[0x34] = b'>';
    map[0x35] = b'?';

    map[0x39] = b' ';

    map
};

fn handle_keyboard_scancode(scancode: u8) {
    if scancode == 0xE0 {
        EXT_PREFIX.store(true, Ordering::Relaxed);
        return;
    }

    let is_ext = EXT_PREFIX.swap(false, Ordering::Relaxed);
    let is_break = (scancode & 0x80) != 0;
    let code = (scancode & 0x7F) as usize;
    let pressed = if is_break { 0u8 } else { 1u8 };

    if is_ext {
        let doom_key = match code {
            0x48 => Some(0xad), // KEY_UPARROW
            0x50 => Some(0xaf), // KEY_DOWNARROW
            0x4D => Some(0xae), // KEY_RIGHTARROW
            0x4B => Some(0xac), // KEY_LEFTARROW
            _ => None,
        };
        if let Some(k) = doom_key {
            crate::arch::keyboard::push_event(pressed, k);
        }

        if !is_break {
            match code {
                0x48 => {
                    crate::tty::push_byte(0x1B);
                    crate::tty::push_byte(b'[');
                    crate::tty::push_byte(b'A');
                    KEYS.fetch_add(1, Ordering::Relaxed);
                }
                0x50 => {
                    crate::tty::push_byte(0x1B);
                    crate::tty::push_byte(b'[');
                    crate::tty::push_byte(b'B');
                    KEYS.fetch_add(1, Ordering::Relaxed);
                }
                0x4D => {
                    crate::tty::push_byte(0x1B);
                    crate::tty::push_byte(b'[');
                    crate::tty::push_byte(b'C');
                    KEYS.fetch_add(1, Ordering::Relaxed);
                }
                0x4B => {
                    crate::tty::push_byte(0x1B);
                    crate::tty::push_byte(b'[');
                    crate::tty::push_byte(b'D');
                    KEYS.fetch_add(1, Ordering::Relaxed);
                }
                0x53 => {
                    crate::tty::push_byte(0x7F);
                    KEYS.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
        }
        return;
    }

    match scancode {
        0x2A | 0x36 => {
            SHIFT_ACTIVE.store(true, Ordering::Relaxed);
            crate::arch::keyboard::push_event(1, 0xb6); // KEY_RSHIFT
            return;
        }
        0xAA | 0xB6 => {
            SHIFT_ACTIVE.store(false, Ordering::Relaxed);
            crate::arch::keyboard::push_event(0, 0xb6);
            return;
        }
        0x1D => {
            CTRL_ACTIVE.store(true, Ordering::Relaxed);
            crate::arch::keyboard::push_event(1, 0xa3); // KEY_FIRE
            return;
        }
        0x9D => {
            CTRL_ACTIVE.store(false, Ordering::Relaxed);
            crate::arch::keyboard::push_event(0, 0xa3);
            return;
        }
        _ => {}
    }

    if code < 128 {
        let base_char = SCANCODE_MAP[code];
        if base_char != 0 {
            crate::arch::keyboard::push_event(pressed, base_char);
        } else if code == 0x01 {
            crate::arch::keyboard::push_event(pressed, 27); // ESC
        }
    }

    if is_break || code >= 128 {
        return;
    }

    let shift = SHIFT_ACTIVE.load(Ordering::Relaxed);
    let ctrl = CTRL_ACTIVE.load(Ordering::Relaxed);

    let ascii = if shift {
        SCANCODE_SHIFT_MAP[code]
    } else {
        SCANCODE_MAP[code]
    };

    if ascii != 0 {
        let ch = if ctrl {
            match ascii {
                b'a'..=b'z' => ascii - b'a' + 1,
                b'A'..=b'Z' => ascii - b'A' + 1,
                _ => ascii,
            }
        } else {
            ascii
        };
        crate::tty::push_byte(ch);
        KEYS.fetch_add(1, Ordering::Relaxed);
    }
}
