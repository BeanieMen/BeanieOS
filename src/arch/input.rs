use core::sync::atomic::{AtomicUsize, Ordering};

use crate::arch::lock::InterruptMutex;

// Bounded: this is on the interrupt path, where the heap may not exist yet.
// Overflow is counted in `DROPPED` rather than hidden.
const CAPACITY: usize = 256;

// Monotonic, so a test can read "did anything overflow" without racing.
static DROPPED: AtomicUsize = AtomicUsize::new(0);

// Head and tail only move forward; `count` separates "empty" from "wrapped".
struct Ring {
    slots: [char; CAPACITY],
    head: usize,
    tail: usize,
    count: usize,
}

impl Ring {
    const fn new() -> Self {
        Ring {
            slots: ['\0'; CAPACITY],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    // Returns false when full so the caller can count the loss.
    fn push(&mut self, key: char) -> bool {
        if self.count == CAPACITY {
            return false;
        }

        self.slots[self.head] = key;
        self.head = (self.head + 1) % CAPACITY;
        self.count += 1;
        true
    }

    // Oldest key, or None when empty.
    fn pop(&mut self) -> Option<char> {
        if self.count == 0 {
            return None;
        }

        let key = self.slots[self.tail];
        self.slots[self.tail] = '\0';
        self.tail = (self.tail + 1) % CAPACITY;
        self.count -= 1;
        Some(key)
    }
}

static PENDING: InterruptMutex<Ring> = InterruptMutex::new(Ring::new());

// Called from the keyboard IRQ. Never blocks: the lock is held for two stores.
pub(crate) fn push_key(key: char) {
    let dropped = {
        let mut ring = PENDING.lock();

        if ring.push(key) { 0 } else { 1 }
    };

    if dropped > 0 {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

fn pop_key() -> Option<char> {
    PENDING.lock().pop()
}

// Keys dropped since boot because the queue was full.
fn dropped_keys() -> usize {
    DROPPED.load(Ordering::Relaxed)
}
