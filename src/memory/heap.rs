use linked_list_allocator::LockedHeap;

pub(crate) const HEAP_START: usize = 0x_4444_4444_0000;
pub(crate) const HEAP_SIZE: usize = 1000 * 1024; // 1000 KiB

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub(crate) fn init_allocator() {
    unsafe { ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE) }
}
