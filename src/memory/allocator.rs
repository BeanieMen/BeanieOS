use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{
        FrameAllocator, OffsetPageTable, PageSize, PageTable, PhysFrame, Size4KiB,
    },
};

use multiboot2::{MemoryArea, MemoryAreaType, MemoryMapTag};

// Linker-provided kernel bounds (see linker.ld: kernel_start / kernel_end).
unsafe extern "C" {
    static kernel_start: u8;
    static kernel_end: u8;
}

const PAGE: u64 = Size4KiB::SIZE;

fn align_up(addr: u64) -> u64 {
    (addr + PAGE - 1) & !(PAGE - 1)
}

fn kernel_range() -> (u64, u64) {
    unsafe {
        let start = (&kernel_start as *const u8) as u64;
        let end = (&kernel_end as *const u8) as u64;
        (start, end)
    }
}

fn frame_is_reserved(addr: u64, kstart: u64, kend: u64, mbi_start: u64, mbi_end: u64) -> bool {
    let frame_end = addr + PAGE;
    // 1 mib range is reserved for firmware
    if addr < 0x10_0000 {
        return true;
    }
    // kernel range is reserved
    if addr < kend && frame_end > kstart {
        return true;
    }
    // multiboot2 boot info range is reserved
    if addr < mbi_end && frame_end > mbi_start {
        return true;
    }
    false
}

pub struct Reserved {
    inner: spin::Mutex<ReservedList>,
}

struct ReservedList {
    entries: [crate::hal::Region; MAX_RESERVED],
    count: usize,
}

impl ReservedList {
    const fn new() -> Self {
        ReservedList {
            entries: [crate::hal::Region::new(0, 0); MAX_RESERVED],
            count: 0,
        }
    }

    fn reset(&mut self) {
        self.count = 0;
    }

    fn push(&mut self, region: crate::hal::Region) {
        if self.count >= MAX_RESERVED {
            return;
        }

        self.entries[self.count] = region;
        self.count += 1;
    }

    fn contains(&self, addr: u64) -> bool {
        self.entries[..self.count]
            .iter()
            .any(|region| region.contains(addr))
    }
}

const MAX_RESERVED: usize = 32;

impl Reserved {
    pub const fn new() -> Self {
        Reserved {
            inner: spin::Mutex::new(ReservedList::new()),
        }
    }

    pub fn snapshot(&self) {
        let mut guard = self.inner.lock();
        guard.reset();

        crate::hal::hal()
            .dma
            .copy_reserved(|region| guard.push(region));
    }

    pub fn contains(&self, addr: u64) -> bool {
        self.inner.lock().contains(addr)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().count
    }
}

pub static RESERVED: Reserved = Reserved::new();

/// Never hands out the kernel image, the boot information, low memory, or a frame
/// a device owns.
pub struct BumpAllocator<'a> {
    areas: &'a [MemoryArea],
    area_idx: usize,
    curr_addr: u64,
    kstart: u64,
    kend: u64,
    mbi_start: u64,
    mbi_end: u64,
    reserved: &'static Reserved,
}

impl<'a> BumpAllocator<'a> {
    pub unsafe fn init(memory_map: &'a MemoryMapTag, mbi_start: u64, mbi_end: u64) -> Self {
        let (kstart, kend) = kernel_range();
        let areas = memory_map.memory_areas();
        let mut area_idx = 0;
        let mut curr_addr = 0;
        for (i, area) in areas.iter().enumerate() {
            if area.typ() != MemoryAreaType::Available {
                continue;
            }
            let addr = align_up(area.start_address());
            if addr + PAGE <= area.end_address() {
                area_idx = i;
                curr_addr = addr;
                break;
            }
        }
        Self {
            areas,
            area_idx,
            curr_addr,
            kstart,
            kend,
            mbi_start,
            mbi_end,
            reserved: &RESERVED,
        }
    }

    fn advance_area(&mut self) -> bool {
        self.area_idx += 1;

        let Some(area) = self.areas.get(self.area_idx) else {
            return false;
        };

        self.curr_addr = align_up(area.start_address());
        true
    }
}

unsafe impl FrameAllocator<Size4KiB> for BumpAllocator<'_> {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        while self.area_idx < self.areas.len() {
            let area = &self.areas[self.area_idx];
            if area.typ() != MemoryAreaType::Available {
                self.advance_area();
                continue;
            }
            let area_end = area.end_address();
            // Align cursor to area start if we just entered it.
            let area_start = align_up(area.start_address());
            if self.curr_addr < area_start {
                self.curr_addr = area_start;
            }
            while self.curr_addr + PAGE <= area_end {
                let addr = self.curr_addr;
                self.curr_addr += PAGE;
                if frame_is_reserved(addr, self.kstart, self.kend, self.mbi_start, self.mbi_end) {
                    continue;
                }
                if self.reserved.contains(addr) {
                    continue;
                }
                return Some(PhysFrame::containing_address(PhysAddr::new(addr)));
            }
            if !self.advance_area() {
                break;
            }
        }
        None
    }
}

fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();

    let virt = physical_memory_offset + phys.as_u64();

    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

pub unsafe fn mapper(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}
