use x86_64::instructions::port::Port;

use crate::arch::interrupts::consts::{PS2_DATA_PORT, PS2_STATUS_PORT};

const STATUS_OUTPUT_FULL: u8 = 1 << 0;

const KEYBOARD_ACK: u8 = 0xFA;
const KEYBOARD_ENABLE_SCANNING: u8 = 0xF4;

const POLL_LIMIT: u32 = 100_000;

fn status() -> u8 {
    unsafe { Port::<u8>::new(PS2_STATUS_PORT).read() }
}

fn read_data() -> u8 {
    unsafe { Port::<u8>::new(PS2_DATA_PORT).read() }
}

fn send_keyboard_command(byte: u8) {
    unsafe { Port::<u8>::new(PS2_DATA_PORT).write(byte) }
}

fn wait_for(predicate: impl Fn(u8) -> bool) -> bool {
    for _ in 0..POLL_LIMIT {
        if predicate(status()) {
            return true;
        }

        core::hint::spin_loop();
    }

    false
}

fn wait_output_empty() -> bool {
    wait_for(|status| status & STATUS_OUTPUT_FULL == 0)
}

fn wait_output_full() -> bool {
    wait_for(|status| status & STATUS_OUTPUT_FULL != 0)
}

pub(crate) fn init() {
    let mut drained = 0;

    while drained < POLL_LIMIT && wait_output_full() {
        read_data();
        drained += 1;
    }

    if !wait_output_empty() {
        crate::kwarn!("keyboard: output buffer never drained");
        return;
    }

    send_keyboard_command(KEYBOARD_ENABLE_SCANNING);

    if !wait_output_full() {
        crate::kwarn!("keyboard: controller did not answer the enable command");
        return;
    }

    let ack = read_data();

    if ack != KEYBOARD_ACK {
        crate::kwarn!("keyboard: enable answered {ack:#04x} instead of an ACK");
        return;
    }

    crate::kinfo!("keyboard: scanning enabled");
}
