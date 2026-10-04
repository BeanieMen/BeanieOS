use linked_list_allocator::LockedHeap;

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 1000 * 1024; // 1000 KiB

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Hands the heap to the global allocator. Only valid once `MMU::init_heap` has
/// mapped `HEAP_START..HEAP_START + HEAP_SIZE`.
pub fn init_allocator() {
    unsafe { ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE) }
}
