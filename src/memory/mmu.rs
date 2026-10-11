use multiboot2::MemoryMapTag;
use x86_64::VirtAddr;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{PageTableFlags, PhysFrame};

use crate::arch::lock::InterruptMutex;
use crate::memory::allocator::{alloc_frame, init_frames, kernel_phys_range};
use crate::memory::heap::{HEAP_SIZE, HEAP_START};
use crate::paging::{
    ENTRIES_PER_TABLE, ENTRY_SIZE, PAGE_SIZE, PTE_ADDR_MASK, PTE_HUGE, PTE_LEAF_MASK,
    PTE_PRESENT, PTE_USER, PTE_WRITABLE, phys_to_direct,
};

pub(crate) const KERNEL_RW: PageTableFlags =
    PageTableFlags::from_bits_truncate(crate::paging::PTE_KERNEL_RW);

const P1_LEVEL: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryError {
    OutOfFrames,
}

impl core::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("out of frames")
    }
}

fn index(virt: u64, level: usize) -> u64 {
    match level {
        0 => (virt >> 39) & 0x1ff,
        1 => (virt >> 30) & 0x1ff,
        2 => (virt >> 21) & 0x1ff,
        _ => (virt >> 12) & 0x1ff,
    }
}

unsafe fn read_entry(table: u64, index: u64) -> u64 {
    unsafe { core::ptr::read_volatile((table + index * ENTRY_SIZE) as *const u64) }
}

unsafe fn write_entry(table: u64, index: u64, value: u64) {
    unsafe { core::ptr::write_volatile((table + index * ENTRY_SIZE) as *mut u64, value) }
}

fn new_table() -> Result<u64, MemoryError> {
    let frame = alloc_frame().ok_or(MemoryError::OutOfFrames)?;
    let base = frame.start_address().as_u64();

    let virt = phys_to_direct(base);

    unsafe { core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE as usize) };

    Ok(base)
}

fn split_huge_at(slot: u64) -> Result<u64, MemoryError> {
    let base = new_table()?;

    let old = unsafe { core::ptr::read_volatile(slot as *const u64) };
    let leaf = old & PTE_LEAF_MASK;
    let huge_base = old & PTE_ADDR_MASK;

    for i in 0..ENTRIES_PER_TABLE {
        unsafe {
            write_entry(base, i, (huge_base + i * PAGE_SIZE) | leaf);
        }
    }

    let replacement = base | PTE_PRESENT | PTE_WRITABLE | leaf;

    unsafe { core::ptr::write_volatile(slot as *mut u64, replacement) };

    x86_64::instructions::tlb::flush(VirtAddr::new(huge_base));

    Ok(base)
}

pub(crate) struct Mapper {
    root: PhysFrame,
}

impl Mapper {
    pub(crate) const fn new(root: PhysFrame) -> Self {
        Mapper { root }
    }

    fn base(&self) -> u64 {
        self.root.start_address().as_u64()
    }

    fn walk(&self, virt: u64, flags: PageTableFlags) -> Result<u64, MemoryError> {
        let mut table = self.base();

        for level in 0..P1_LEVEL as usize {
            let slot = table + index(virt, level) * ENTRY_SIZE;
            let entry = unsafe { read_entry(table, index(virt, level)) };

            table = if entry & PTE_PRESENT == 0 {
                let child = new_table()?;

                let user = flags.bits() & PTE_USER;

                unsafe {
                    write_entry(
                        table,
                        index(virt, level),
                        child | PTE_PRESENT | PTE_WRITABLE | user,
                    )
                };

                child
            } else if entry & PTE_HUGE != 0 {
                split_huge_at(slot)?
            } else {
                entry & PTE_ADDR_MASK
            };
        }

        Ok(table)
    }

    pub(crate) fn map(
        &self,
        virt: u64,
        frame: PhysFrame,
        flags: PageTableFlags,
    ) -> Result<(), MemoryError> {
        let virt = virt & !(PAGE_SIZE - 1);

        let table = self.walk(virt, flags)?;

        let leaf = index(virt, P1_LEVEL as usize);

        unsafe { write_entry(table, leaf, frame.start_address().as_u64() | flags.bits()) };

        x86_64::instructions::tlb::flush_all();

        Ok(())
    }

    pub(crate) fn map_fresh(
        &self,
        virt: u64,
        frames: u64,
        flags: PageTableFlags,
    ) -> Result<(), MemoryError> {
        for i in 0..frames {
            let frame = alloc_frame().ok_or(MemoryError::OutOfFrames)?;

            self.map(virt + i * PAGE_SIZE, frame, flags)?;
        }

        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MemoryInfo {
    pub framebuffer: Option<(u64, u64)>,
    pub kernel: (u64, u64),
    pub boot_info: (u64, u64),
    pub heap: Option<(u64, u64)>,
}

static MEMORY: InterruptMutex<MemoryInfo> = InterruptMutex::new(MemoryInfo {
    framebuffer: None,
    kernel: (0, 0),
    boot_info: (0, 0),
    heap: None,
});

pub(crate) fn memory_info() -> MemoryInfo {
    *MEMORY.lock()
}

pub(crate) fn set_framebuffer_range(start: u64, end: u64) {
    MEMORY.lock().framebuffer = Some((start, end));
}

pub(crate) fn set_heap_range(start: u64, end: u64) {
    MEMORY.lock().heap = Some((start, end));
}

pub(crate) struct MMU<'a> {
    mapper: Mapper,
    _marker: core::marker::PhantomData<&'a ()>,
}

impl<'a> MMU<'a> {
    pub(crate) unsafe fn boot(
        memory_map: &'a MemoryMapTag,
        mbi_start: u64,
        mbi_end: u64,
    ) -> Self {
        {
            let mut info = MEMORY.lock();
            info.kernel = kernel_phys_range();
            info.boot_info = (mbi_start, mbi_end);
        }

        init_frames(memory_map, mbi_start, mbi_end);

        let (root, _) = Cr3::read();

        Self {
            mapper: Mapper::new(root),
            _marker: core::marker::PhantomData,
        }
    }

    pub(crate) fn mapper(&self) -> &Mapper {
        &self.mapper
    }

    pub(crate) fn init_heap(&mut self) -> Result<(), MemoryError> {
        let pages = (HEAP_SIZE as u64).div_ceil(PAGE_SIZE);

        self.mapper
            .map_fresh(HEAP_START as u64, pages, KERNEL_RW)?;

        crate::memory::heap::init_allocator();

        let range = self.heap_range();
        set_heap_range(range.0, range.1);

        Ok(())
    }

    pub(crate) fn heap_range(&self) -> (u64, u64) {
        (HEAP_START as u64, HEAP_START as u64 + HEAP_SIZE as u64)
    }
}
