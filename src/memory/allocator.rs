use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB},
};

use multiboot2::{MemoryArea, MemoryAreaType, MemoryMapTag};

// Linker-provided kernel bounds (see linker.ld: kernel_start / kernel_end).
unsafe extern "C" {
    static kernel_start: u8;
    static kernel_end: u8;
}

fn kernel_range() -> (u64, u64) {
    unsafe {
        let start = (&kernel_start as *const u8) as u64;
        let end = (&kernel_end as *const u8) as u64;
        (start, end)
    }
}

fn frame_is_reserved(addr: u64, kstart: u64, kend: u64, mbi_start: u64, mbi_end: u64) -> bool {
    let frame_end = addr + 4096;
    if addr < 0x10_0000 {
        return true;
    }
    if addr < kend && frame_end > kstart {
        return true;
    }
    if addr < mbi_end && frame_end > mbi_start {
        return true;
    }
    false
}

/// Frame allocator over the Multiboot2 memory map that never hands out
/// frames belonging to:
/// - the kernel ELF itself (kernel_start..kernel_end)
/// - the Multiboot2 boot information (mbi_start..mbi_end)
/// - low memory below 1 MiB (BIOS / real-mode / VGA hole)
///
/// Without this filtering the high-memory mappings (heap at
/// 0x4444_4444_0000, framebuffer at 0x5555_5555_0000) allocate page-table
/// frames from inside the kernel or the MBI and corrupt them. That shows
/// up as page faults, heap corruption, or boot-allocator panics
/// ("High memory allocator: Out of memory" style failures when the map is
/// consumed by overlapping allocations).
///
/// NOTE: this must not use the heap itself (no Vec/Box) because it runs
/// before `init_heap`. It is a simple O(n) cursor over the memory map.
pub struct Multiboot2FrameAllocator<'a> {
    areas: &'a [MemoryArea],
    area_idx: usize,
    curr_addr: u64,
    kstart: u64,
    kend: u64,
    mbi_start: u64,
    mbi_end: u64,
}

impl<'a> Multiboot2FrameAllocator<'a> {
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
            let mut addr = area.start_address();
            addr = (addr + 0xfff) & !0xfff;
            if addr + 4096 <= area.end_address() {
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
}

unsafe impl FrameAllocator<Size4KiB> for Multiboot2FrameAllocator<'_> {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        while self.area_idx < self.areas.len() {
            let area = &self.areas[self.area_idx];
            if area.typ() != MemoryAreaType::Available {
                self.area_idx += 1;
                if self.area_idx < self.areas.len() {
                    let mut addr = self.areas[self.area_idx].start_address();
                    addr = (addr + 0xfff) & !0xfff;
                    self.curr_addr = addr;
                }
                continue;
            }
            let area_end = area.end_address();
            // Align cursor to area start if we just entered it.
            let mut area_start = area.start_address();
            area_start = (area_start + 0xfff) & !0xfff;
            if self.curr_addr < area_start {
                self.curr_addr = area_start;
            }
            while self.curr_addr + 4096 <= area_end {
                let addr = self.curr_addr;
                self.curr_addr += 4096;
                if frame_is_reserved(addr, self.kstart, self.kend, self.mbi_start, self.mbi_end) {
                    continue;
                }
                return Some(PhysFrame::containing_address(PhysAddr::new(addr)));
            }
            self.area_idx += 1;
            if self.area_idx < self.areas.len() {
                let mut addr = self.areas[self.area_idx].start_address();
                addr = (addr + 0xfff) & !0xfff;
                self.curr_addr = addr;
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
