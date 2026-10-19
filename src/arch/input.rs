use alloc::vec::Vec;

use crate::arch::lock::InterruptMutex;
use crate::task::identity::ThreadId;

static WAITERS: InterruptMutex<Vec<ThreadId>> = InterruptMutex::new(Vec::new());

pub(crate) fn push_key(_key: char) {
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
