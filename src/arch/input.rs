use alloc::vec::Vec;

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::arch::lock::InterruptMutex;
use crate::task::identity::ThreadId;

const CAPACITY: usize = 256;

static DROPPED: AtomicUsize = AtomicUsize::new(0);

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

    fn push(&mut self, key: char) -> bool {
        if self.count == CAPACITY {
            return false;
        }

        self.slots[self.head] = key;
        self.head = (self.head + 1) % CAPACITY;
        self.count += 1;
        true
    }

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

static WAITERS: InterruptMutex<Vec<ThreadId>> = InterruptMutex::new(Vec::new());

pub(crate) fn push_key(key: char) {
    let dropped = {
        let mut ring = PENDING.lock();

        if ring.push(key) { 0 } else { 1 }
    };

    if dropped > 0 {
        DROPPED.fetch_add(1, Ordering::Relaxed);
        return;
    }

    wake_one();
}


pub(crate) fn add_waiter(id: ThreadId) {
    WAITERS.lock().push(id);
}

pub(crate) fn remove_waiter(id: ThreadId) {
    WAITERS.lock().retain(|waiter| *waiter != id);
}

fn wake_one() {
    let waiter = { WAITERS.lock().pop() };

    if let Some(id) = waiter {
        crate::task::scheduler::wake(id);
    }
}

pub(crate) fn pop_key() -> Option<char> {
    PENDING.lock().pop()
}

pub(crate) fn pending() -> usize {
    PENDING.lock().count
}

pub(crate) fn dropped_keys() -> usize {
    DROPPED.load(Ordering::Relaxed)
}