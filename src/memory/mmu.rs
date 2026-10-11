use core::sync::atomic::{AtomicU64, Ordering};

use multiboot2::MemoryMapTag;
use x86_64::PhysAddr;
use x86_64::VirtAddr;
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::{PageTableFlags, PhysFrame};

use crate::kerror;
use crate::memory::allocator::{alloc_frame, free_frame, init_frames};
use crate::memory::heap::{HEAP_SIZE, HEAP_START};

pub(crate) use crate::paging::{
    ENTRIES_PER_TABLE, ENTRY_SIZE, LOW_1MIB, PAGE_SIZE, PTE_ADDR_MASK, PTE_BOOT_IDENTITY, PTE_HUGE,
    PTE_LEAF_MASK, PTE_PRESENT,
};

// One definition of each flag set, so a mapping cannot disagree with `boot.rs`
// about what "writable" means.
pub(crate) const KERNEL_RW: PageTableFlags =
    PageTableFlags::from_bits_truncate(crate::paging::PTE_KERNEL_RW);
pub(crate) const USER_RW: PageTableFlags =
    PageTableFlags::from_bits_truncate(crate::paging::PTE_USER_RW);

const P1_LEVEL: u8 = 3;

// Why a mapping or an allocation did not happen. `Option`/bool made "no frame
// left" and "nothing to do" the same value at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryError {
    OutOfFrames,
    NotMapped,
    AlreadyMapped,
    // A mapping call was given a non-page-aligned address.
    Unaligned,
}

impl core::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            MemoryError::OutOfFrames => "out of frames",
            MemoryError::NotMapped => "not mapped",
            MemoryError::AlreadyMapped => "already mapped",
            MemoryError::Unaligned => "address is not page aligned",
        };
        f.write_str(s)
    }
}

#[allow(clippy::upper_case_acronyms)]
use crate::paging::{PTE_USER, PTE_WRITABLE};

// A handle, not a second implementation: `map_one` is the mapping engine and
// `AddressSpace` is the owner.
pub(crate) struct MMU<'a> {
    space: AddressSpace,
    _marker: core::marker::PhantomData<&'a ()>,
}

fn index(virt: u64, level: usize) -> u64 {
    match level {
        0 => (virt >> 39) & 0x1ff,
        1 => (virt >> 30) & 0x1ff,
        2 => (virt >> 21) & 0x1ff,
        _ => (virt >> 12) & 0x1ff,
    }
}

// SAFETY: `table` must be a page-aligned, mapped, writable physical address.
unsafe fn read_entry(table: u64, index: u64) -> u64 {
    unsafe { core::ptr::read_volatile((table + index * ENTRY_SIZE) as *const u64) }
}

// SAFETY: as `read_entry`, and the table must not be shared with a live mapper.
unsafe fn write_entry(table: u64, index: u64, value: u64) {
    unsafe { core::ptr::write_volatile((table + index * ENTRY_SIZE) as *mut u64, value) }
}

// A zeroed frame to stand in for a missing page table.
fn new_table() -> Result<u64, MemoryError> {
    let frame = alloc_frame().ok_or(MemoryError::OutOfFrames)?;
    let base = frame.start_address().as_u64();

    unsafe { core::ptr::write_bytes(base as *mut u8, 0, PAGE_SIZE as usize) };

    Ok(base)
}

// Replaces the 2 MiB page whose level 2 entry sits at `slot` with a 512-entry
// table mapping the same physical range one page at a time.
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

// Maps one 4 KiB page with an explicit P4 -> P1 walk. `OffsetPageTable` required
// the virtual page and its table in the same 1 GiB, which is why the heap could
// not be mapped at all; walking by hand puts no such relationship between them.
fn map_one(
    root: u64,
    virt: u64,
    frame: PhysFrame,
    flags: PageTableFlags,
) -> Result<(), MemoryError> {
    let mut table = root;

    // Levels 0, 1 and 2 are P4, P3 and P2. P1 is the leaf and is written after.
    for level in 0..P1_LEVEL as usize {
        let slot = table + index(virt, level) * ENTRY_SIZE;
        let entry = unsafe { read_entry(table, index(virt, level)) };

        table = if entry & PTE_PRESENT == 0 {
            let child = new_table()?;

            // A table is writable, and user-accessible only when the leaf will
            // be. The leaf carries the real restriction.
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

    let leaf = index(virt, P1_LEVEL as usize);

    unsafe { write_entry(table, leaf, frame.start_address().as_u64() | flags.bits()) };

    x86_64::instructions::tlb::flush_all();

    Ok(())
}

// Frame backing `virt`. Panics when unmapped: a masked-off zero here becomes
// physical address 0 in a driver's descriptor, where a read returns floating bus
// data, a write is dropped, and nothing faults.
fn translate_in(root: u64, virt: u64) -> PhysFrame {
    let mut table = root;

    for level in 0..P1_LEVEL {
        let entry = unsafe { read_entry(table, index(virt, level as usize)) };

        assert!(
            entry & PTE_PRESENT != 0,
            "translate: {virt:#x} has no level {level} entry"
        );

        // PS is a huge-page flag at P2 and P1 only; at P4/P3 bit 7 is PAT.
        if level > 0 && entry & PTE_HUGE != 0 {
            let shift = if level == 1 { 30 } else { 21 };
            let page = (1 << shift) - 1;

            return PhysFrame::containing_address(PhysAddr::new(
                (entry & PTE_ADDR_MASK & page) + (virt & page),
            ));
        }

        table = entry & PTE_ADDR_MASK;
    }

    PhysFrame::containing_address(PhysAddr::new(table))
}

// Every frame the tree at `table` holds, children first.
fn release_tree(table: u64, level: u8) {
    for i in 0..ENTRIES_PER_TABLE {
        let entry = unsafe { read_entry(table, i) };

        if entry & PTE_PRESENT == 0 {
            continue;
        }

        let child = entry & PTE_ADDR_MASK;

        // P1 entries are leaves; above that a non-huge entry is a pointer to
        // the next table down, and its pages have to be released too.
        if level >= P1_LEVEL || entry & PTE_HUGE != 0 {
            free_frame(child);
        } else {
            release_tree(child, level + 1);
        }
    }

    free_frame(table);
}

// A process's page tables.
pub(crate) struct AddressSpace {
    pub root: PhysFrame,
    owned: bool,
}

static KERNEL_CR3: AtomicU64 = AtomicU64::new(0);

fn remember_kernel_cr3() {
    let (frame, _) = Cr3::read();
    KERNEL_CR3.store(frame.start_address().as_u64(), Ordering::Release);
}

pub(crate) fn kernel_cr3() -> PhysFrame {
    PhysFrame::containing_address(PhysAddr::new(KERNEL_CR3.load(Ordering::Acquire)))
}

// The only place CR3 is written, so there is one path to audit rather than five.
pub(crate) fn switch_cr3(root: PhysFrame) {
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

impl<'a> MMU<'a> {
    pub(crate) unsafe fn boot(memory_map: &'a MemoryMapTag, mbi_start: u64, mbi_end: u64) -> Self {
        init_frames(memory_map, mbi_start, mbi_end);

        let (frame, _) = Cr3::read();

        remember_kernel_cr3();

        Self {
            space: AddressSpace {
                root: frame,
                owned: false,
            },
            _marker: core::marker::PhantomData,
        }
    }

    fn root(&self) -> u64 {
        self.space.root_addr()
    }

    pub(crate) fn map_range(
        &mut self,
        virt: u64,
        phys: u64,
        size: u64,
        flags: PageTableFlags,
    ) -> Result<usize, MemoryError> {
        let pages = size.div_ceil(PAGE_SIZE);

        for i in 0..pages {
            let v = (virt + i * PAGE_SIZE) & !(PAGE_SIZE - 1);
            let p = (phys + i * PAGE_SIZE) & !(PAGE_SIZE - 1);

            let frame = PhysFrame::containing_address(PhysAddr::new(p));

            map_one(self.root(), v, frame, flags).map_err(|e| {
                kerror!("    map failed at {p:#x}: {e}");
                e
            })?;
        }

        Ok(pages as usize)
    }

    pub(crate) fn init_heap(&mut self) -> Result<(), MemoryError> {
        let pages = (HEAP_SIZE as u64).div_ceil(PAGE_SIZE);

        for i in 0..pages {
            let frame = alloc_frame().ok_or(MemoryError::OutOfFrames)?;

            map_one(
                self.root(),
                HEAP_START as u64 + i * PAGE_SIZE,
                frame,
                KERNEL_RW,
            )?;
        }

        crate::memory::heap::init_allocator();

        Ok(())
    }

    pub(crate) fn heap_range(&self) -> (u64, u64) {
        (HEAP_START as u64, HEAP_START as u64 + HEAP_SIZE as u64)
    }

    pub(crate) fn activate(&self) {
        self.space.activate();
    }

    pub(crate) fn translate(&self, virt: u64) -> PhysFrame {
        translate_in(self.root(), virt)
    }

    pub(crate) fn root_frame(&self) -> PhysFrame {
        self.space.root
    }
}

pub(crate) const USER_BASE: u64 = 0x0040_0000;
pub(crate) const USER_STACK_TOP: u64 = 0x0030_0000;
const USER_SIZE: u64 = 0x0002_0000;
pub(crate) const USER_DATA: u64 = 0x0050_0000;
pub(crate) const USER_START: u64 = 0x0020_0000;
pub(crate) const USER_END: u64 = 0x0060_0000;

impl AddressSpace {
    pub(crate) fn root_addr(&self) -> u64 {
        self.root.start_address().as_u64()
    }

    pub(crate) fn new() -> Result<Self, MemoryError> {
        let frame = alloc_frame().ok_or(MemoryError::OutOfFrames)?;

        unsafe { core::ptr::write_bytes(frame.start_address().as_u64() as *mut u8, 0, 4096) };

        Ok(AddressSpace {
            root: frame,
            owned: true,
        })
    }

    pub(crate) fn map(
        &mut self,
        virt: u64,
        target: PhysFrame,
        flags: PageTableFlags,
    ) -> Result<(), MemoryError> {
        map_one(self.root_addr(), virt & !(PAGE_SIZE - 1), target, flags)
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

    fn map_kernel(&mut self, virt: u64, size: u64) -> Result<u64, MemoryError> {
        self.map_fresh(virt, size, KERNEL_RW)
    }

    fn map_user(&mut self, virt: u64, size: u64) -> Result<u64, MemoryError> {
        self.map_fresh(virt, size, USER_RW)
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
