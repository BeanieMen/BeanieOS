use alloc::{format, vec::Vec};

use x86_64::structures::paging::PageTableFlags;

use super::dma::Dma;
use crate::arch::lock::InterruptMutex;
use crate::kdebug;
use crate::memory::mmu::{KERNEL_RW, MMU};
use crate::memory::pool::Area;

// Where MMIO is mapped. Zero: at the BAR's own physical address.
pub(crate) const MMIO_BASE: u64 = 0;

// Uncached and write-through, so device reads and writes go straight through.
const MMIO_FLAGS: PageTableFlags = PageTableFlags::from_bits_truncate(
    KERNEL_RW.bits() | PageTableFlags::NO_CACHE.bits() | PageTableFlags::WRITE_THROUGH.bits(),
);

#[derive(Clone)]
pub(crate) struct Mapping {
    pub va: usize,
    pub size: u64,
}

pub(crate) struct Mmapped {
    mappings: InterruptMutex<Vec<Mapping>>,
}

impl Mmapped {
    pub(crate) const fn new() -> Self {
        Mmapped {
            mappings: InterruptMutex::new(Vec::new()),
        }
    }

    pub(crate) fn mapping_at(&self, va: usize) -> Option<Mapping> {
        self.mappings
            .lock()
            .iter()
            .find(|m| va >= m.va && va < m.va + m.size as usize)
            .cloned()
    }

    pub(crate) fn map(
        &self,
        mmu: &mut MMU<'_>,
        dma: &Dma,
        owner: &str,
        phys: u64,
        size: u64,
    ) -> Option<usize> {
        let start = phys & !0xfff;
        let end = (phys + size.max(0x1000) + 0xfff) & !0xfff;

        mmu.map_range(MMIO_BASE + start, start, end - start, MMIO_FLAGS)
            .ok()?;

        dma.reserve(Area::new(start, end));

        self.mappings.lock().push(Mapping {
            va: MMIO_BASE as usize + start as usize,
            size: end - start,
        });

        kdebug!("  mmio {owner} {start:#x}..{end:#x}");

        Some(MMIO_BASE as usize + start as usize)
    }

    pub(crate) fn map_all_bars(
        &self,
        mmu: &mut MMU<'_>,
        dma: &Dma,
        routes: &crate::hal::pci::Routes,
    ) {
        for route in routes.iter() {
            let Some((phys, size)) = route.bar5_info() else {
                continue;
            };

            let owner = format!("{}", route.address);
            self.map(mmu, dma, &owner, phys, size);
        }
    }

    pub(crate) fn reserve_all_bars(&self, dma: &Dma, routes: &crate::hal::pci::Routes) {
        for route in routes.iter() {
            let Some((phys, size)) = route.bar5_info() else {
                continue;
            };

            let start = phys & !0xfff;
            let end = (phys + size.max(0x1000) + 0xfff) & !0xfff;

            kdebug!("  reserve mmio {} {start:#x}..{end:#x}", route.address);
            dma.reserve(Area::new(start, end));
        }
    }
}

impl Default for Mmapped {
    fn default() -> Self {
        Self::new()
    }
}
