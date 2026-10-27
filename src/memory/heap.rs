use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use linked_list_allocator::Heap;
use crate::arch::lock::InterruptMutex;

pub(crate) const HEAP_START: usize = crate::paging::HEAP_BASE as usize;
pub(crate) const HEAP_SIZE: usize = 64 * 1024 * 1024;

pub struct InterruptLockedHeap(InterruptMutex<Heap>);

impl InterruptLockedHeap {
    pub const fn empty() -> Self {
        InterruptLockedHeap(InterruptMutex::new(Heap::empty()))
    }
}

unsafe impl GlobalAlloc for InterruptLockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0
            .lock()
            .allocate_first_fit(layout)
            .ok()
            .map_or(core::ptr::null_mut(), |allocation| allocation.as_ptr())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            self.0
                .lock()
                .deallocate(NonNull::new_unchecked(ptr), layout);
        }
    }
}

#[global_allocator]
static ALLOCATOR: InterruptLockedHeap = InterruptLockedHeap::empty();

pub(crate) fn init_allocator() {
    unsafe { ALLOCATOR.0.lock().init(HEAP_START as *mut u8, HEAP_SIZE) }
}
