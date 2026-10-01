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

const PAGE: u64 = Size4KiB::SIZE as u64;

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

/// Never hands out the kernel image, the boot information, or low memory.
pub struct BumpAllocator<'a> {
    areas: &'a [MemoryArea],
    area_idx: usize,
    curr_addr: u64,
    kstart: u64,
    kend: u64,
    mbi_start: u64,
    mbi_end: u64,
}

impl<'a> BumpAllocator<'a> {
    pub unsafe fn init(memory_map: &'a MemoryMapTag, mbi_start: u64, mbi_end: u64) -> Self {
        let (kstart, kend) = kernel_range();
        let areas = memory_map.memory_areas();
        // Find first Available area to start from.
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
        }
    }

    /// Point the cursor at the start of the area after the current one, and
    /// say whether there was one.
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

pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);

    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}
