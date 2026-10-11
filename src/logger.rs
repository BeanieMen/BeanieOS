use core::sync::atomic::{AtomicU8, Ordering};

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
    const fn as_str(&self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    pub(crate) const fn color(&self) -> Color {
        match self {
            Level::Error => Color::Red,
            Level::Warn => Color::Yellow,
            Level::Info => Color::Green,
            Level::Debug => Color::Blue,
            Level::Trace => Color::Cyan,
        }
    }

    const fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Level::Error,
            1 => Level::Warn,
            2 => Level::Info,
            3 => Level::Debug,
            _ => Level::Trace,
        }
    }
}

const MAX_LEVEL: Level = Level::Trace;

static LEVEL_FILTER: AtomicU8 = AtomicU8::new(Level::Info as u8);

fn set_level(level: Level) {
    LEVEL_FILTER.store(level as u8, Ordering::Relaxed);
}

pub(crate) fn level() -> Level {
    Level::from_bits(LEVEL_FILTER.load(Ordering::Relaxed))
}

pub fn _log(level: Level, module: &str, args: core::fmt::Arguments) {
    if level > MAX_LEVEL || level as u8 > LEVEL_FILTER.load(Ordering::Relaxed) {
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
