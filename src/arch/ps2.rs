use core::sync::atomic::{AtomicU64, Ordering};
use pc_keyboard::{layouts, DecodedKey, HandleControl, KeyCode, KeyState, PS2Keyboard, ScancodeSet1};
use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::arch::interrupts::consts::{PS2_DATA_PORT, PS2_STATUS_PORT};

const STATUS_OUTPUT_FULL: u8 = 1 << 0;
const STATUS_INPUT_FULL: u8 = 1 << 1;
const STATUS_AUX_DATA: u8 = 1 << 5;

const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_ENABLE_PORT2: u8 = 0xA8;
const CMD_ENABLE_PORT1: u8 = 0xAE;
const CMD_WRITE_PORT2: u8 = 0xD4;

const MOUSE_SET_DEFAULTS: u8 = 0xF6;
const MOUSE_ENABLE_REPORTING: u8 = 0xF4;
const KEYBOARD_ENABLE_SCANNING: u8 = 0xF4;

const POLL_LIMIT: u32 = 100_000;

static KEYS: AtomicU64 = AtomicU64::new(0);

static KEYBOARD: Mutex<PS2Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(PS2Keyboard::new(
    ScancodeSet1::new(),
    layouts::Us104Key,
    HandleControl::MapLettersToUnicode,
));

struct MouseState {
    phase: u8,
    bytes: [u8; 3],
}

impl MouseState {
    const fn new() -> Self {
        Self {
            phase: 0,
            bytes: [0; 3],
        }
    }
}

static MOUSE: Mutex<MouseState> = Mutex::new(MouseState::new());

fn status() -> u8 {
    unsafe { Port::<u8>::new(PS2_STATUS_PORT).read() }
}

fn read_data() -> u8 {
    unsafe { Port::<u8>::new(PS2_DATA_PORT).read() }
}

fn wait_input_empty() -> bool {
    for _ in 0..POLL_LIMIT {
        if status() & STATUS_INPUT_FULL == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn wait_output_full() -> bool {
    for _ in 0..POLL_LIMIT {
        if status() & STATUS_OUTPUT_FULL != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn write_command(cmd: u8) {
    wait_input_empty();
    unsafe { Port::<u8>::new(PS2_STATUS_PORT).write(cmd) };
}

fn write_data(data: u8) {
    wait_input_empty();
    unsafe { Port::<u8>::new(PS2_DATA_PORT).write(data) };
}

fn send_mouse_command(cmd: u8) -> u8 {
    write_command(CMD_WRITE_PORT2);
    write_data(cmd);
    if wait_output_full() {
        read_data()
    } else {
        0
    }
}

pub(crate) fn init() {
    for _ in 0..1000 {
        if status() & STATUS_OUTPUT_FULL != 0 {
            read_data();
        } else {
            break;
        }
    }

    write_command(CMD_READ_CONFIG);
    if !wait_output_full() {
        crate::kwarn!("ps2: failed to read configuration");
        return;
    }
    let mut config = read_data();

    config |= (1 << 0) | (1 << 1) | (1 << 6);
    config &= !((1 << 4) | (1 << 5));

    write_command(CMD_WRITE_CONFIG);
    write_data(config);

    write_command(CMD_ENABLE_PORT2);
    write_command(CMD_ENABLE_PORT1);

    write_data(KEYBOARD_ENABLE_SCANNING);
    if wait_output_full() {
        let ack = read_data();
        crate::kinfo!("ps2: keyboard ack {ack:#04x}");
    }

    let ack1 = send_mouse_command(MOUSE_SET_DEFAULTS);
    let ack2 = send_mouse_command(MOUSE_ENABLE_REPORTING);
    crate::kinfo!("ps2: mouse acks {ack1:#04x}, {ack2:#04x}");

    write_command(CMD_READ_CONFIG);
    if wait_output_full() {
        let _ = read_data();
    }

    for _ in 0..1000 {
        if status() & STATUS_OUTPUT_FULL != 0 {
            read_data();
        } else {
            break;
        }
    }
}

pub(crate) fn on_interrupt() {
    while status() & STATUS_OUTPUT_FULL != 0 {
        let st = status();
        let data = read_data();

        if st & STATUS_AUX_DATA != 0 {
            handle_mouse_byte(data);
        } else {
            handle_keyboard_scancode(data);
        }
    }
}

fn handle_mouse_byte(byte: u8) {
    let mut mouse = MOUSE.lock();
    match mouse.phase {
        0 => {
            if byte & 0x08 != 0 {
                mouse.bytes[0] = byte;
                mouse.phase = 1;
            }
        }
        1 => {
            mouse.bytes[1] = byte;
            mouse.phase = 2;
        }
        2 => {
            mouse.bytes[2] = byte;
            mouse.phase = 0;

            let flags = mouse.bytes[0];
            let raw_x = mouse.bytes[1];
            let raw_y = mouse.bytes[2];

            let dx = if flags & 0x10 != 0 {
                (raw_x as i32) - 256
            } else {
                raw_x as i32
            };

            let dy = if flags & 0x20 != 0 {
                (raw_y as i32) - 256
            } else {
                raw_y as i32
            };

            let left = (flags & 0x01) as i32;
            let right = ((flags & 0x02) >> 1) as i32;
            let middle = ((flags & 0x04) >> 2) as i32;

            crate::input::event::push_rel(crate::input::event::REL_X, dx);
            crate::input::event::push_rel(crate::input::event::REL_Y, -dy);
            crate::input::event::push_button(crate::input::event::BTN_LEFT, left);
            crate::input::event::push_button(crate::input::event::BTN_RIGHT, right);
            crate::input::event::push_button(crate::input::event::BTN_MIDDLE, middle);
            crate::input::event::push_syn();

            crate::input::mouse::push_packet(flags, raw_x, raw_y);
        }
        _ => mouse.phase = 0,
    }
}

fn handle_keyboard_scancode(scancode: u8) {
    let mut keyboard = KEYBOARD.lock();
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        let pressed_val: i32 = if key_event.state == KeyState::Down { 1 } else { 0 };
        let code = keycode_to_event_code(key_event.code);
        if code != 0 {
            crate::input::event::push_key(code, pressed_val);
        }

        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(ch) => {
                    let mut buf = [0u8; 4];
                    let str_bytes = ch.encode_utf8(&mut buf).as_bytes();
                    for &b in str_bytes {
                        crate::tty::push_byte(b);
                        KEYS.fetch_add(1, Ordering::Relaxed);
                    }
                }
                DecodedKey::RawKey(raw) => {
                    match raw {
                        KeyCode::ArrowUp => {
                            crate::tty::push_byte(0x1B);
                            crate::tty::push_byte(b'[');
                            crate::tty::push_byte(b'A');
                            KEYS.fetch_add(1, Ordering::Relaxed);
                        }
                        KeyCode::ArrowDown => {
                            crate::tty::push_byte(0x1B);
                            crate::tty::push_byte(b'[');
                            crate::tty::push_byte(b'B');
                            KEYS.fetch_add(1, Ordering::Relaxed);
                        }
                        KeyCode::ArrowRight => {
                            crate::tty::push_byte(0x1B);
                            crate::tty::push_byte(b'[');
                            crate::tty::push_byte(b'C');
                            KEYS.fetch_add(1, Ordering::Relaxed);
                        }
                        KeyCode::ArrowLeft => {
                            crate::tty::push_byte(0x1B);
                            crate::tty::push_byte(b'[');
                            crate::tty::push_byte(b'D');
                            KEYS.fetch_add(1, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn keycode_to_event_code(code: KeyCode) -> u16 {
    match code {
        KeyCode::Escape => 1,
        KeyCode::Key1 => 2,
        KeyCode::Key2 => 3,
        KeyCode::Key3 => 4,
        KeyCode::Key4 => 5,
        KeyCode::Key5 => 6,
        KeyCode::Key6 => 7,
        KeyCode::Key7 => 8,
        KeyCode::Key8 => 9,
        KeyCode::Key9 => 10,
        KeyCode::Key0 => 11,
        KeyCode::OemMinus => 12,
        KeyCode::OemPlus => 13,
        KeyCode::Backspace => 14,
        KeyCode::Tab => 15,
        KeyCode::Q => 16,
        KeyCode::W => 17,
        KeyCode::E => 18,
        KeyCode::R => 19,
        KeyCode::T => 20,
        KeyCode::Y => 21,
        KeyCode::U => 22,
        KeyCode::I => 23,
        KeyCode::O => 24,
        KeyCode::P => 25,
        KeyCode::Oem4 => 26,
        KeyCode::Oem6 => 27,
        KeyCode::Return => 28,
        KeyCode::LControl => 29,
        KeyCode::A => 30,
        KeyCode::S => 31,
        KeyCode::D => 32,
        KeyCode::F => 33,
        KeyCode::G => 34,
        KeyCode::H => 35,
        KeyCode::J => 36,
        KeyCode::K => 37,
        KeyCode::L => 38,
        KeyCode::Oem1 => 39,
        KeyCode::Oem3 => 40,
        KeyCode::Oem8 => 41,
        KeyCode::LShift => 42,
        KeyCode::Oem5 => 43,
        KeyCode::Z => 44,
        KeyCode::X => 45,
        KeyCode::C => 46,
        KeyCode::V => 47,
        KeyCode::B => 48,
        KeyCode::N => 49,
        KeyCode::M => 50,
        KeyCode::OemComma => 51,
        KeyCode::OemPeriod => 52,
        KeyCode::Oem2 => 53,
        KeyCode::RShift => 54,
        KeyCode::LAlt => 56,
        KeyCode::Spacebar => 57,
        KeyCode::NumpadEnter => 96,
        KeyCode::ArrowUp => 103,
        KeyCode::ArrowLeft => 105,
        KeyCode::ArrowRight => 106,
        KeyCode::ArrowDown => 108,
        KeyCode::RControl => 97,
        _ => 0,
    }
}
