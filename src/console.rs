use core::fmt;

use crate::graphics::framebuffer::WRITER;

struct SerialPort;

impl core::fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        use x86_64::instructions::port::Port;

        let mut port = unsafe { Port::new(0x3f8) };

        for byte in s.bytes() {
            unsafe { port.write(byte) };
        }

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
