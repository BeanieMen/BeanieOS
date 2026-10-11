use core::sync::atomic::{AtomicU64, Ordering};

use multiboot2::MemoryMapTag;
use x86_64::PhysAddr;
use x86_64::VirtAddr;
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{PageTableFlags, PhysFrame};

use crate::arch::lock::InterruptMutex;
use crate::memory::allocator::{alloc_frame, free_frame, init_frames, kernel_phys_range};
use crate::memory::heap::{HEAP_SIZE, HEAP_START};
use crate::paging::{
    DIRECT_MAP_P4_INDEX, ENTRIES_PER_TABLE, ENTRY_SIZE, KERNEL_P4_INDEX, KERNEL_VIRTUAL_BASE,
    PAGE_SIZE, PTE_ADDR_MASK, PTE_HUGE, PTE_LEAF_MASK, PTE_PRESENT, PTE_USER, PTE_WRITABLE,
    phys_to_direct,
};

pub(crate) const KERNEL_RW: PageTableFlags =
    PageTableFlags::from_bits_truncate(crate::paging::PTE_KERNEL_RW);
pub(crate) const USER_RW: PageTableFlags =
    PageTableFlags::from_bits_truncate(crate::paging::PTE_USER_RW);

const SHARED_P4_SLOTS: [u64; 2] = [KERNEL_P4_INDEX, DIRECT_MAP_P4_INDEX];

unsafe extern "C" {
    static P4_TABLE: u8;
}

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

fn translate_in(root: u64, virt: u64) -> PhysFrame {
    let mut frame = root;

    for level in 0..P1_LEVEL {
        let table = phys_to_direct(frame);
        let entry = unsafe { read_entry(table, index(virt, level as usize)) };

        assert!(
            entry & PTE_PRESENT != 0,
            "translate: {virt:#x} has no level {level} entry"
        );

        if level > 0 && entry & PTE_HUGE != 0 {
            let shift = if level == 1 { 30 } else { 21 };
            let page = (1 << shift) - 1;

            return PhysFrame::containing_address(PhysAddr::new(
                (entry & PTE_ADDR_MASK & page) + (virt & page),
            ));
        }

        frame = entry & PTE_ADDR_MASK;
    }

    let table = phys_to_direct(frame);
    let leaf = unsafe { read_entry(table, index(virt, P1_LEVEL as usize)) };

    assert!(
        leaf & PTE_PRESENT != 0,
        "translate: {virt:#x} has no leaf entry"
    );

    PhysFrame::containing_address(PhysAddr::new(leaf & PTE_ADDR_MASK))
}

fn is_shared_slot(index: u64) -> bool {
    index >= DIRECT_MAP_P4_INDEX
}

/// Frees every frame `table` owns, children first.
///
/// Stops at the shared P4 slots: those subtrees belong to the kernel and to no
/// process, and walking into one would free the kernel image along with every
/// process's view of it.
fn release_tree(root: u64, level: u8) {
    let table = phys_to_direct(root);

    for i in 0..ENTRIES_PER_TABLE {
        if level == 0 && is_shared_slot(i) {
            continue;
        }

        let entry = unsafe { read_entry(table, i) };

        if entry & PTE_PRESENT == 0 {
            continue;
        }

        let child = entry & PTE_ADDR_MASK;

        if level >= P1_LEVEL || entry & PTE_HUGE != 0 {
            free_frame(child);
        } else {
            release_tree(child, level + 1);
        }
    }

    free_frame(table);
}

pub(crate) struct AddressSpace {
    pub root: PhysFrame,
    owned: bool,
}

static KERNEL_CR3: AtomicU64 = AtomicU64::new(0);

pub(crate) fn remember_kernel_cr3() {
    let (frame, _) = Cr3::read();
    KERNEL_CR3.store(frame.start_address().as_u64(), Ordering::Release);
}

pub(crate) fn kernel_cr3() -> PhysFrame {
    let addr = KERNEL_CR3.load(Ordering::Acquire);
    if addr == 0 {
        Cr3::read().0
    } else {
        PhysFrame::containing_address(PhysAddr::new(addr))
    }
}

pub(crate) fn switch_cr3(root: PhysFrame) {
    if root.start_address().as_u64() == 0 {
        return;
    }
    let (current, _) = Cr3::read();
    if current != root {
        unsafe { Cr3::write(root, Cr3Flags::empty()) };
    }
}

pub(crate) fn current_cr3() -> PhysFrame {
    Cr3::read().0
}

pub(crate) fn activate_root(root: PhysFrame) {
    switch_cr3(root);
}

impl AddressSpace {
    pub(crate) fn root_addr(&self) -> u64 {
        self.root.start_address().as_u64()
    }

    fn walk(&self, virt: u64, flags: PageTableFlags) -> Result<u64, MemoryError> {
        let mut frame = self.root_addr();

        for level in 0..P1_LEVEL as usize {
            let table = phys_to_direct(frame);
            let slot = table + index(virt, level) * ENTRY_SIZE;
            let entry = unsafe { read_entry(table, index(virt, level)) };

            frame = if entry & PTE_PRESENT == 0 {
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

        Ok(frame)
    }

    pub(crate) fn new() -> Result<Self, MemoryError> {
        let frame = alloc_frame().ok_or(MemoryError::OutOfFrames)?;

        let virt = phys_to_direct(frame.start_address().as_u64());

        unsafe { core::ptr::write_bytes(virt as *mut u8, 0, PAGE_SIZE as usize) };

        Ok(AddressSpace {
            root: frame,
            owned: true,
        })
    }

    /// A fresh address space sharing the kernel's two upper P4 entries rather
    /// than copying the image. Sharing is what keeps process creation cheap, and
    /// why `release_tree` has to skip those slots.
    pub(crate) fn new_with_kernel() -> Result<Self, MemoryError> {
        let space = Self::new()?;

        let boot_p4 = (&raw const P4_TABLE) as u64;

        // A root frame is a physical address and nothing maps that into the
        // higher half, so it has to go through the direct map to be writable.
        let target = phys_to_direct(space.root_addr());

        for slot in SHARED_P4_SLOTS {
            unsafe {
                write_entry(target, slot, read_entry(boot_p4, slot));
            }
        }

        Ok(space)
    }

    pub(crate) fn from_root(root: PhysFrame) -> Self {
        AddressSpace { root, owned: true }
    }

    pub(crate) fn map(
        &mut self,
        virt: u64,
        target: PhysFrame,
        flags: PageTableFlags,
    ) -> Result<(), MemoryError> {
        let virt = virt & !(PAGE_SIZE - 1);

        let frame = self.walk(virt, flags)?;
        let table = phys_to_direct(frame);

        let leaf = index(virt, P1_LEVEL as usize);

        unsafe { write_entry(table, leaf, target.start_address().as_u64() | flags.bits()) };

        x86_64::instructions::tlb::flush_all();

        Ok(())
    }

    pub(crate) fn map_fresh(
        &mut self,
        virt: u64,
        size: u64,
        flags: PageTableFlags,
    ) -> Result<u64, MemoryError> {
        let pages = size.div_ceil(PAGE_SIZE);

        for i in 0..pages {
            let frame = alloc_frame().ok_or(MemoryError::OutOfFrames)?;

            self.map(virt + i * PAGE_SIZE, frame, flags)?;
        }

        Ok(pages)
    }

    pub(crate) fn translate(&self, virt: u64) -> PhysFrame {
        translate_in(self.root_addr(), virt)
    }

    pub(crate) fn activate(&self) {
        switch_cr3(self.root);
    }

    fn is_active(&self) -> bool {
        current_cr3() == self.root
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }

        if self.is_active() {
            activate_root(kernel_cr3());
        }

        release_tree(self.root_addr(), 0);
    }
}

/// Lowest user address, and the top of the canonical lower half.
pub(crate) const USER_START: u64 = 0x0000_0000_0000_0000;
pub(crate) const USER_END: u64 = KERNEL_VIRTUAL_BASE - 1;

/// Code, stack and data, low enough to be well clear of the firmware's.
pub(crate) const USER_BASE: u64 = 0x0040_0000;
pub(crate) const USER_STACK_TOP: u64 = 0x0030_0000;
pub(crate) const USER_DATA: u64 = 0x0050_0000;

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
        remember_kernel_cr3();

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
