use alloc::vec::Vec;

use crate::arch::lock::InterruptMutex;
use crate::task::identity::ThreadId;

static WAITERS: InterruptMutex<Vec<ThreadId>> = InterruptMutex::new(Vec::new());

pub(crate) fn push_key(_key: char) {
    wake_waiters();
}

pub(crate) fn add_waiter(id: ThreadId) {
    let mut list = WAITERS.lock();
    if !list.contains(&id) {
        list.push(id);
    }
}

pub(crate) fn remove_waiter(id: ThreadId) {
    WAITERS.lock().retain(|waiter| *waiter != id);
}

pub(crate) fn wake_waiters() {
    let waiters: Vec<ThreadId> = {
        let mut list = WAITERS.lock();
        list.drain(..).collect()
    };

    for id in waiters {
        crate::task::scheduler::wake(id);
    }
}

fn wake_one() {
    let waiter = { WAITERS.lock().pop() };

    if let Some(id) = waiter {
        crate::task::scheduler::wake(id);
    }
}
