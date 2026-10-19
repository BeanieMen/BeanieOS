use core::fmt;

use crate::graphics::framebuffer::WRITER;

struct SerialPort;

impl SerialPort {
    fn write_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        unsafe {
            core::arch::asm!(
                "rep outsb",
                inout("rsi") bytes.as_ptr() => _,
                inout("rcx") bytes.len() => _,
                in("dx") 0x3f8u16,
                options(nostack, preserves_flags)
            );
        }
    }
}

pub fn write_user_bytes(bytes: &[u8]) {
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        WRITER.lock().write_bytes(bytes);
        SerialPort.write_bytes(bytes);
    });
}

impl core::fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_bytes(s.as_bytes());
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| {
        WRITER.lock().write_fmt(args).unwrap();
        SerialPort.write_fmt(args).unwrap();
    })
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::console::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };

    ($($arg:tt)*) => {
        $crate::print!("{}\n", format_args!($($arg)*))
    };
}
