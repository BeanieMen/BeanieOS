use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use x86_64::instructions::interrupts;

pub(crate) struct InterruptMutex<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for InterruptMutex<T> {}
unsafe impl<T: Send> Send for InterruptMutex<T> {}

impl<T> InterruptMutex<T> {
    pub(crate) const fn new(value: T) -> Self {
        InterruptMutex {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(value),
        }
    }

    pub(crate) fn lock(&self) -> InterruptGuard<'_, T> {
        let restore = interrupts::are_enabled();

        // Mask first, then test: an interrupt landing between the two would
        // deadlock against the context it interrupted.
        interrupts::disable();

        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }

        InterruptGuard {
            data: unsafe { &mut *self.data.get() },
            locked: &self.locked,
            restore,
        }
    }
}

pub(crate) struct InterruptGuard<'a, T> {
    data: &'a mut T,
    locked: &'a AtomicBool,
    restore: bool,
}

impl<T> Deref for InterruptGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.data
    }
}

impl<T> DerefMut for InterruptGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.data
    }
}

impl<T> Drop for InterruptGuard<'_, T> {
    fn drop(&mut self) {
        self.locked.store(false, Ordering::Release);

        // Enabling unconditionally would open a window inside a caller that
        // deliberately masked, and the next lock there would deadlock.
        if self.restore {
            interrupts::enable();
        }
    }
}
