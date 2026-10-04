use spin::Mutex;
use x86_64::instructions::interrupts;

/// Unread keys. The handler writes, the main loop drains.
const PENDING_MAX: usize = 64;

static PENDING: Mutex<[char; PENDING_MAX]> = Mutex::new(['\0'; PENDING_MAX]);

pub fn push_key(key: char) {
    interrupts::without_interrupts(|| {
        let mut ring = PENDING.lock();
        if let Some(slot) = ring.iter_mut().find(|slot| **slot == '\0') {
            *slot = key;
        }
    })
}

pub fn pop_key() -> Option<char> {
    interrupts::without_interrupts(|| {
        let mut ring = PENDING.lock();
        let slot = ring.iter_mut().find(|slot| **slot != '\0')?;
        let key = *slot;
        *slot = '\0';
        Some(key)
    })
}
