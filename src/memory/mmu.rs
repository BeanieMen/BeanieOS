use multiboot2::MemoryMapTag;
use x86_64::PhysAddr;
use x86_64::VirtAddr;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame,
    mapper::MapToError,
};

use crate::memory::allocator::BumpAllocator;
use crate::memory::heap::{HEAP_SIZE, HEAP_START};
use crate::print;
use crate::println;

pub const PRESENT: u64 = 1 << 0;
pub const WRITABLE: u64 = 1 << 1;
pub const USER: u64 = 1 << 2;
pub const WRITE_THROUGH: u64 = 1 << 3;
pub const NO_CACHE: u64 = 1 << 4;
pub const ACCESSED: u64 = 1 << 5;
pub const DIRTY: u64 = 1 << 6;
pub const HUGE_PAGE: u64 = 1 << 7;
pub const GLOBAL: u64 = 1 << 8;

pub const FRAME_MASK: u64 = 0x000f_ffff_ffff_f000;

const ENTRY_SIZE: u64 = 8;
const PAGE: u64 = 4096;

pub const LEVELS: [&str; 4] = ["P4", "P3", "P2", "P1"];

/// One level of a walk: the table the entry lives in, and the entry itself.
#[derive(Clone, Copy)]
pub struct Step {
    pub table: u64,
    pub index: u64,
    pub entry: u64,
}

impl Step {
    pub fn is_present(&self) -> bool {
        self.entry & PRESENT != 0
    }

    pub fn is_huge(&self) -> bool {
        self.entry & HUGE_PAGE != 0
    }

    pub fn frame(&self) -> u64 {
        self.entry & FRAME_MASK
    }
}

/// An address space, the frames it is built from, and the heap inside it.
pub struct MMU<'a> {
    root: u64,
    mapper: OffsetPageTable<'a>,
    frames: BumpAllocator<'a>,
}

impl<'a> MMU<'a> {
    pub unsafe fn boot(memory_map: &'a MemoryMapTag, mbi_start: u64, mbi_end: u64) -> Self {
        let (frame, _) = Cr3::read();
        let root = frame.start_address().as_u64();

        // The map is an identity map, so a table's physical address is also the
        // address the mapper has to dereference to reach it.
        let p4 = unsafe { &mut *(root as *mut PageTable) };
        let mapper = unsafe { OffsetPageTable::new(p4, VirtAddr::new(0)) };
        let frames = unsafe { BumpAllocator::init(memory_map, mbi_start, mbi_end) };

        Self {
            root,
            mapper,
            frames,
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

    pub fn read_entry(table: u64, index: u64) -> u64 {
        unsafe { core::ptr::read_volatile((table + index * ENTRY_SIZE) as *const u64) }
    }

    pub fn write_entry(table: u64, index: u64, value: u64) {
        unsafe { core::ptr::write_volatile((table + index * ENTRY_SIZE) as *mut u64, value) }
    }

    pub fn walk(&self, virt: u64) -> [Option<Step>; 4] {
        let mut steps = [None; 4];
        let mut table = self.root;

        for level in 0..4 {
            let index = Self::index(virt, level);
            let entry = Self::read_entry(table, index);

            steps[level] = Some(Step {
                table,
                index,
                entry,
            });

            if entry & PRESENT == 0 || entry & HUGE_PAGE != 0 {
                break;
            }

            table = entry & FRAME_MASK;
        }

        steps
    }

    fn print_flags(entry: u64) {
        for (bit, name) in [
            (PRESENT, "p"),
            (WRITABLE, "rw"),
            (USER, "us"),
            (HUGE_PAGE, "ps"),
        ] {
            if entry & bit != 0 {
                print!("{name} ");
            }
        }
    }

    pub fn describe(&self, label: &str, virt: u64) {
        let steps = self.walk(virt);

        print!("  {label:<9} {virt:#014x}");

        for level in 0..4 {
            let Some(step) = steps[level] else {
                print!(" {} absent", LEVELS[level]);
                break;
            };

            print!(
                " {}[t={:#x} i={:#04x}]={:#014x}",
                LEVELS[level], step.table, step.index, step.entry
            );

            if !step.is_present() {
                print!(" absent");
                break;
            }

            if step.is_huge() {
                print!(" huge leaf {:#x} ", step.frame());
                Self::print_flags(step.entry);
                break;
            }

            if level == 3 {
                print!(" leaf {:#x} ", step.frame());
                Self::print_flags(step.entry);
            }
        }

        println!();
    }

    pub fn survey(&self) {
        println!("mmu root {:#x}", self.root);

        self.describe("zero", 0);
        self.describe("kernel", 0x10_0000);
        self.describe("edge", 0x20_0000);
        self.describe("heap", HEAP_START as u64);
    }

    /// The level 2 table entry covering `virt`, if it sits under a huge page.
    fn pd_for(&self, virt: u64) -> Option<(u64, u64)> {
        let mut table = self.root;

        for level in 0..2 {
            let index = Self::index(virt, level);
            let entry = Self::read_entry(table, index);

            if entry & PRESENT == 0 || entry & HUGE_PAGE != 0 {
                return None;
            }

            table = entry & FRAME_MASK;
        }

        Some((table, Self::index(virt, 2)))
    }

    /// Replaces the 2 MiB page covering `virt` with 512 4 KiB entries.
    fn split_huge(&mut self, virt: u64) -> bool {
        let Some((pd, index)) = self.pd_for(virt) else {
            return false;
        };

        let entry = Self::read_entry(pd, index);

        if entry & PRESENT == 0 || entry & HUGE_PAGE == 0 {
            return false;
        }

        let Some(frame) = self.frames.allocate_frame() else {
            return false;
        };

        let table = frame.start_address().as_u64();
        let leaf = entry & (PRESENT | WRITABLE);
        let base = entry & FRAME_MASK;

        for i in 0..512 {
            Self::write_entry(table, i, (base + i * PAGE) | leaf);
        }

        Self::write_entry(pd, index, table | leaf);

        x86_64::instructions::tlb::flush(VirtAddr::new(base));

        true
    }

    /// Maps `[virt, virt + size)` onto `[phys, phys + size)`.
    ///
    /// The boot map covers the low 8 GiB with 2 MiB pages
    pub fn map_range(
        &mut self,
        virt: u64,
        phys: u64,
        size: u64,
        flags: PageTableFlags,
    ) -> Option<usize> {
        let pages = size.div_ceil(PAGE);

        for i in 0..pages {
            let v = (virt + i * PAGE) & !(PAGE - 1);
            let p = (phys + i * PAGE) & !(PAGE - 1);

            self.split_huge(v);

            let page: Page = Page::containing_address(VirtAddr::new(v));
            let frame = PhysFrame::containing_address(PhysAddr::new(p));

            let result = unsafe { self.mapper.map_to(page, frame, flags, &mut self.frames) };

            match result {
                Ok(_) | Err(MapToError::PageAlreadyMapped(_)) => {}
                Err(e) => {
                    println!("    map failed at {p:#x}: {e:?}");
                    return None;
                }
            }
        }

        Some(pages as usize)
    }

    /// Maps the heap and hands it to the global allocator.
    pub fn init_heap(&mut self) {
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        let range = Page::range_inclusive(
            Page::containing_address(VirtAddr::new(HEAP_START as u64)),
            Page::containing_address(VirtAddr::new((HEAP_START + HEAP_SIZE - 1) as u64)),
        );

        for page in range {
            let frame = self
                .frames
                .allocate_frame()
                .expect("no frame left for the heap");
            let result = unsafe { self.mapper.map_to(page, frame, flags, &mut self.frames) };

            result.expect("could not map the heap").flush();
        }

        crate::memory::heap::init_allocator();
    }
}
