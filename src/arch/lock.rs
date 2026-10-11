use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use x86_64::instructions::interrupts;

// `const`-constructible, so it can still live in a `static`.
pub(crate) struct InterruptMutex<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

// The critical section is protected by the IF flag, not by the lock word: a
// context that can be interrupted on this CPU cannot be running here. The
// `T: Send` bound carries that argument to the compiler -- moving a `T` between
// CPUs would not be safe under this scheme, and this makes the compiler reject
// it rather than leaving it to a reader to notice.
unsafe impl<T: Send> Sync for InterruptMutex<T> {}
unsafe impl<T: Send> Send for InterruptMutex<T> {}

impl<T> InterruptMutex<T> {
    pub(crate) const fn new(value: T) -> Self {
        InterruptMutex {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(value),
        }
    }

    // Takes the lock with interrupts masked; the guard puts IF back as it found it.
    pub(crate) fn lock(&self) -> InterruptGuard<'_, T> {
        let restore = interrupts::are_enabled();

        // Mask first, then test. Reversing these two lines is the whole bug this type
        // exists to prevent: an interrupt landing between test and mask would
        // deadlock against the context it interrupted.
        interrupts::disable();

        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // Interrupts are off, so the one context that could take the lock cannot
            // interrupt this. On a uniprocessor it exits on the first pass.
            core::hint::spin_loop();
        }

        InterruptGuard {
            data: unsafe { &mut *self.data.get() },
            locked: &self.locked,
            restore,
        }
    }
}

// The guard returned by `InterruptMutex::lock`.
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
        // Release before unmasking, so the next holder is not held up by our masking.
        self.locked.store(false, Ordering::Release);

        // Restore IF only if it was set on entry. Enabling unconditionally would
        // open a window inside a caller that deliberately masked, and the next
        // lock taken there would deadlock.
        if self.restore {
            interrupts::enable();
        }
    }
}
