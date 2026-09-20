use x86_64::structures::idt::InterruptDescriptorTable;

use super::consts::{KEYBOARD_VECTOR, PS2_DATA_PORT, SPURIOUS_VECTOR};
use super::pic;

extern "x86-interrupt" fn spurious_interrupt_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    let mut port = x86_64::instructions::port::Port::new(PS2_DATA_PORT);
    let scancode: u8 = unsafe { port.read() };

    if let Some(c) = scancode_to_ascii(scancode) {
        crate::print!("{}", c);
    }

    unsafe {
        pic::eoi();
    }
}

pub(crate) fn register_vectors(idt: &mut InterruptDescriptorTable) {
    idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_interrupt_handler);
    idt[SPURIOUS_VECTOR].set_handler_fn(spurious_interrupt_handler);
}

// evils map putting this piece of code here because i dont know where to put it right now

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

    // QWERTY row
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

    // ASDF row
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

    // ZXCV row
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

    // Space
    map[0x39] = b' ';

    map
};

fn scancode_to_ascii(scancode: u8) -> Option<char> {
    if scancode & 0x80 != 0 {
        return None;
    }

    let ascii = SCANCODE_MAP[scancode as usize];

    if ascii == 0 {
        None
    } else {
        Some(ascii as char)
    }
}
