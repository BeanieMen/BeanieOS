use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{
        FrameAllocator,
        Mapper,
        OffsetPageTable,
        PageTable,
        PhysFrame,
        Size4KiB,
    },
};

use crate::{Multiboot2MemoryMap, println};

pub struct Multiboot2FrameAllocator {
    memory_map: &'static Multiboot2MemoryMap,
    next: usize,
}

impl Multiboot2FrameAllocator {
    pub unsafe fn init(
        memory_map: &'static Multiboot2MemoryMap,
    ) -> Self {
        Self {
            memory_map,
            next: 0,
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for Multiboot2FrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.memory_map
            .entries()
            .filter(|entry| entry.entry_type == 1)
            .flat_map(|entry| {
                (entry.base_addr..entry.base_addr + entry.length)
                    .step_by(4096)
            })
            .map(|addr| {
                PhysFrame::containing_address(
                    PhysAddr::new(addr)
                )
            })
            .nth(self.next);

        self.next += 1;

        frame
    }
}

fn active_level_4_table(
    physical_memory_offset: VirtAddr,
) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();

    let virt = physical_memory_offset + phys.as_u64();

    let page_table_ptr: *mut PageTable =
        virt.as_mut_ptr();

    unsafe {
        &mut *page_table_ptr
    }
}

pub unsafe fn init(
    physical_memory_offset: VirtAddr,
) -> OffsetPageTable<'static> {
    let level_4_table =
        unsafe { active_level_4_table(physical_memory_offset) };

        unsafe {
        OffsetPageTable::new(
            level_4_table,
            physical_memory_offset,
        )
    }
}