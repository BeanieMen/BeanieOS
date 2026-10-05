use spin::Mutex;

use crate::graphics::framebuffer::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    pub const fn color(&self) -> Color {
        match self {
            Level::Error => Color::Red,
            Level::Warn => Color::Yellow,
            Level::Info => Color::Green,
            Level::Debug => Color::Blue,
            Level::Trace => Color::Cyan,
        }
    }
}

pub const MAX_LEVEL: Level = Level::Trace;

static LEVEL_FILTER: Mutex<Level> = Mutex::new(Level::Info);

pub fn _log(level: Level, module: &str, args: core::fmt::Arguments) {
    if level > MAX_LEVEL || level > *LEVEL_FILTER.lock() {
        return;
    }

    crate::println!("[{}] [{}] {}", level.as_str(), module, args);
}

#[macro_export]
macro_rules! log {
    ($level:expr, $($arg:tt)*) => {
        $crate::logger::_log($level, module_path!(), format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kerror { ($($arg:tt)*) => { $crate::log!($crate::logger::Level::Error, $($arg)*) } }
#[macro_export]
macro_rules! kwarn  { ($($arg:tt)*) => { $crate::log!($crate::logger::Level::Warn,  $($arg)*) } }
#[macro_export]
macro_rules! kinfo  { ($($arg:tt)*) => { $crate::log!($crate::logger::Level::Info,  $($arg)*) } }
#[macro_export]
macro_rules! kdebug { ($($arg:tt)*) => { $crate::log!($crate::logger::Level::Debug, $($arg)*) } }
#[macro_export]
macro_rules! ktrace { ($($arg:tt)*) => { $crate::log!($crate::logger::Level::Trace, $($arg)*) } }
