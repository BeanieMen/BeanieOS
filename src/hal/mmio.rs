use alloc::{format, vec::Vec};
use core::ptr;

use spin::Mutex;
use x86_64::PhysAddr;
use x86_64::VirtAddr;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, PhysFrame, Size4KiB,
    mapper::MapToError,
};

use super::dma::Dma;
use crate::println;

/// Where MMIO is mapped. Zero: at the BAR's own physical address.
pub const MMIO_BASE: u64 = 0;

/// Uncached and write-through, so register reads reach the device and register
/// writes land before a polling loop looks for them.
const MMIO_FLAGS: PageTableFlags = PageTableFlags::from_bits_truncate(
    PageTableFlags::PRESENT.bits()
        | PageTableFlags::WRITABLE.bits()
        | PageTableFlags::NO_CACHE.bits()
        | PageTableFlags::WRITE_THROUGH.bits(),
);

const LARGE_PAGE: u64 = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct Mapping {
    pub va: usize,
    pub size: u64,
}

pub struct Mmapped {
    mapper: Mutex<Option<OffsetPageTable<'static>>>,
    mappings: Mutex<Vec<Mapping>>,
}

impl Mmapped {
    pub const fn new() -> Self {
        Mmapped {
            mapper: Mutex::new(None),
            mappings: Mutex::new(Vec::new()),
        }
    }

    pub fn set_mapper(&self, mapper: OffsetPageTable<'static>) {
        *self.mapper.lock() = Some(mapper);
    }

    pub fn mapping_at(&self, va: usize) -> Option<Mapping> {
        self.mappings
            .lock()
            .iter()
            .find(|m| va >= m.va && va < m.va + m.size as usize)
            .cloned()
    }

    pub fn map(
        &self,
        dma: &Dma,
        owner: &str,
        phys: u64,
        size: u64,
        frames: &mut impl FrameAllocator<Size4KiB>,
    ) -> Option<usize> {
        let start = phys & !0xfff;
        let end = (phys + size.max(0x1000) + 0xfff) & !0xfff;

        let mut guard = self.mapper.lock();
        let mapper = guard.as_mut()?;

        // The boot map covers this range with 2 MiB pages, and `map_to` refuses
        // to descend past a large-page parent, so refine each one first.
        let mut at = start;
        let mut splits = 0;
        while at < end {
            if LargePage::at(VirtAddr::new(at)).split(frames) {
                splits += 1;
            }
            at += LARGE_PAGE;
        }

        let mut at = start;
        while at < end {
            let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(MMIO_BASE + at));
            let frame = PhysFrame::containing_address(PhysAddr::new(at));

            // SAFETY: these frames come from a BAR, so the device drives them.
            let result = unsafe { mapper.map_to(page, frame, MMIO_FLAGS, frames) };

            match result {
                Ok(_) | Err(MapToError::PageAlreadyMapped(_)) => {}
                Err(e) => {
                    println!("    map failed at {at:#x}: {e:?}");
                    return None;
                }
            }

            at += 4096;
        }

        drop(guard);

        dma.reserve(super::dma::Region::new(start, end));

        self.mappings.lock().push(Mapping {
            va: MMIO_BASE as usize + start as usize,
            size: end - start,
        });

        println!("  mmio {owner} {start:#x}..{end:#x} ({splits} large page(s) split)",);

        Some(MMIO_BASE as usize + start as usize)
    }

    /// Maps the register block of every discovered AHCI function.
    pub fn map_all_bars(
        &self,
        dma: &Dma,
        routes: &crate::hal::pci::Routes,
        frames: &mut impl FrameAllocator<Size4KiB>,
    ) {
        for route in routes.iter() {
            let Some((phys, size)) = route.bar5_info() else {
                continue;
            };

            let owner = format!("{}", route.address);
            self.map(dma, &owner, phys, size, frames);
        }
    }

    pub fn reserve_all_bars(&self, dma: &Dma, routes: &crate::hal::pci::Routes) {
        for route in routes.iter() {
            let Some((phys, size)) = route.bar5_info() else {
                continue;
            };

            let start = phys & !0xfff;
            let end = (phys + size.max(0x1000) + 0xfff) & !0xfff;

            println!("  reserve mmio {} {start:#x}..{end:#x}", route.address);
            dma.reserve(super::dma::Region::new(start, end));
        }
    }
}

impl Default for Mmapped {
    fn default() -> Self {
        Self::new()
    }
}

/// The level 2 large page covering an address, and the ability to refine it.
struct LargePage {
    /// Address of the page directory entry describing it.
    pd: u64,
    /// That entry's index within the page directory.
    index: u64,
}

impl LargePage {
    /// Finds the page directory entry covering `virt`, if there is one.
    fn at(virt: VirtAddr) -> Self {
        let addr = virt.as_u64();
        let mut table = root();

        for shift in [39, 30] {
            let e = read_entry(table, (addr >> shift) & 0x1ff);

            if e & PRESENT == 0 {
                return LargePage { pd: 0, index: 0 };
            }

            table = e & FRAME_MASK;
        }

        LargePage {
            pd: table,
            index: (addr >> 21) & 0x1ff,
        }
    }

    fn split(&self, frames: &mut impl FrameAllocator<Size4KiB>) -> bool {
        if self.pd == 0 {
            return false;
        }

        let e = read_entry(self.pd, self.index);

        if e & PRESENT == 0 || e & HUGE_PAGE == 0 {
            return false;
        }

        let Some(frame) = frames.allocate_frame() else {
            return false;
        };

        let table = frame.start_address().as_u64();
        let leaf_flags = e & (PRESENT | WRITABLE);
        let base = e & FRAME_MASK;

        for i in 0..512 {
            write_entry(table, i, (base + i * 4096) | leaf_flags);
        }

        write_entry(self.pd, self.index, table | leaf_flags);

        x86_64::instructions::tlb::flush(VirtAddr::new(base));

        true
    }
}

const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const HUGE_PAGE: u64 = 1 << 7;
const FRAME_MASK: u64 = 0x000f_ffff_ffff_f000;

fn root() -> u64 {
    let (frame, _) = Cr3::read();
    frame.start_address().as_u64()
}

fn read_entry(table: u64, index: u64) -> u64 {
    unsafe { ptr::read_volatile((table + index * 8) as *const u64) }
}

fn write_entry(table: u64, index: u64, value: u64) {
    unsafe { ptr::write_volatile((table + index * 8) as *mut u64, value) }
}
