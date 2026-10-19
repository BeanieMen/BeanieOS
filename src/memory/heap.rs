use linked_list_allocator::LockedHeap;

pub(crate) const HEAP_START: usize = crate::paging::HEAP_BASE as usize;
pub(crate) const HEAP_SIZE: usize = 32 * 1024 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub(crate) fn init_allocator() {
    unsafe { ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE) }
}
