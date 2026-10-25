use multiboot2::{MemoryAreaType, MemoryMapTag};
use x86_64::{
    PhysAddr,
    structures::paging::{FrameAllocator, PhysFrame, Size4KiB},
};

use crate::arch::lock::InterruptMutex;
use crate::memory::pool::{Area, FramePool};
use crate::paging::{DIRECT_MAP_SIZE, LOW_1MIB, kernel_to_phys};

unsafe extern "C" {
    static kernel_start: u8;
    static kernel_end: u8;
}

pub(crate) fn kernel_range() -> (u64, u64) {
    unsafe {
        let start = (&kernel_start as *const u8) as u64;
        let end = (&kernel_end as *const u8) as u64;
        (start, end)
    }
}

pub(crate) fn kernel_phys_range() -> (u64, u64) {
    let (start, end) = kernel_range();
    (kernel_to_phys(start), kernel_to_phys(end))
}

const MAX_RESERVED: usize = 64;

pub(crate) struct Reserved {
    inner: InterruptMutex<ReservedList>,
}

static FRAMEBUFFER: InterruptMutex<Option<(u64, u64)>> = InterruptMutex::new(None);

pub(crate) fn reserve_framebuffer(start: u64, end: u64) {
    *FRAMEBUFFER.lock() = Some((start, end));
}

struct ReservedList {
    entries: [Area; MAX_RESERVED],
    count: usize,
}

impl ReservedList {
    const fn new() -> Self {
        ReservedList {
            entries: [Area::new(0, 0); MAX_RESERVED],
            count: 0,
        }
    }

    fn push(&mut self, region: Area) -> bool {
        if self.count >= MAX_RESERVED || !region.is_valid() {
            return false;
        }

        self.entries[self.count] = region;
        self.count += 1;

        true
    }
}

impl Reserved {
    pub(crate) const fn new() -> Self {
        Reserved {
            inner: InterruptMutex::new(ReservedList::new()),
        }
    }

    pub(crate) fn push(&self, region: Area) -> bool {
        self.inner.lock().push(region)
    }

    pub(crate) fn len(&self) -> usize {
        self.inner.lock().count
    }

    fn avoid_all(&self, pool: &mut FramePool) -> usize {
        let guard = self.inner.lock();
        let mut dropped = 0;

        for region in &guard.entries[..guard.count] {
            if !pool.avoid(*region) {
                dropped += 1;
            }
        }

        dropped
    }

    fn take_framebuffer(&self) -> Option<(u64, u64)> {
        *FRAMEBUFFER.lock()
    }
}

pub(crate) static RESERVED: Reserved = Reserved::new();

pub(crate) struct FrameSource(pub FramePool);

impl FrameSource {
    pub(crate) const fn new() -> Self {
        FrameSource(FramePool::new())
    }
}

unsafe impl FrameAllocator<Size4KiB> for FrameSource {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.0
            .alloc()
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

pub(crate) static FRAMES: InterruptMutex<FrameSource> = InterruptMutex::new(FrameSource::new());

fn is_reachable(end: u64) -> bool {
    end <= DIRECT_MAP_SIZE
}

pub(crate) fn init_frames(memory_map: &MemoryMapTag, mbi_start: u64, mbi_end: u64) {
    let (kstart, kend) = kernel_phys_range();
    let mut guard = FRAMES.lock();
    let pool = &mut guard.0;
    pool.reset();
    for area in memory_map.memory_areas() {
        if area.typ() != MemoryAreaType::Available {
            continue;
        }

        let start = area.start_address();
        let end = area.end_address();

        if start >= DIRECT_MAP_SIZE {
            continue;
        }

        pool.add_area(Area::new(start, end.min(DIRECT_MAP_SIZE)));
    }

    pool.avoid(Area::new(0, LOW_1MIB));
    pool.avoid(Area::new(kstart, kend));
    pool.avoid(Area::new(mbi_start, mbi_end));
    pool.avoid(Area::new(
        crate::paging::HEAP_OFFSET,
        crate::paging::HEAP_OFFSET + crate::memory::heap::HEAP_SIZE as u64,
    ));

    if let Some((start, end)) = RESERVED.take_framebuffer() {
        pool.avoid(Area::new(start, end));
    }

    let dropped = RESERVED.avoid_all(pool);

    assert_eq!(
        dropped, 0,
        "{dropped} device range(s) could not be reserved; their frames would become allocatable RAM"
    );
}

pub(crate) fn alloc_frame() -> Option<PhysFrame> {
    FRAMES.lock().allocate_frame()
}

pub(crate) fn alloc_contiguous(count: usize) -> Option<PhysFrame> {
    FRAMES
        .lock()
        .0
        .alloc_contiguous(count)
        .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
}

pub(crate) fn free_frame(addr: u64) -> bool {
    FRAMES.lock().0.free(addr)
}

pub(crate) fn live_frames() -> usize {
    FRAMES.lock().0.live()
}
