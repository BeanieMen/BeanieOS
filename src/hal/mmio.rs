use alloc::{format, vec::Vec};

use spin::Mutex;
use x86_64::structures::paging::PageTableFlags;

use super::dma::Dma;
use crate::memory::mmu::MMU;
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

#[derive(Clone)]
pub struct Mapping {
    pub va: usize,
    pub size: u64,
}

pub struct Mmapped {
    mappings: Mutex<Vec<Mapping>>,
}

impl Mmapped {
    pub const fn new() -> Self {
        Mmapped {
            mappings: Mutex::new(Vec::new()),
        }
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
        mmu: &mut MMU<'_>,
        dma: &Dma,
        owner: &str,
        phys: u64,
        size: u64,
    ) -> Option<usize> {
        let start = phys & !0xfff;
        let end = (phys + size.max(0x1000) + 0xfff) & !0xfff;

        mmu.map_range(MMIO_BASE + start, start, end - start, MMIO_FLAGS)?;

        dma.reserve(super::dma::Region::new(start, end));

        self.mappings.lock().push(Mapping {
            va: MMIO_BASE as usize + start as usize,
            size: end - start,
        });

        println!("  mmio {owner} {start:#x}..{end:#x}");

        Some(MMIO_BASE as usize + start as usize)
    }

    /// Maps the register block of every discovered AHCI function.
    pub fn map_all_bars(&self, mmu: &mut MMU<'_>, dma: &Dma, routes: &crate::hal::pci::Routes) {
        for route in routes.iter() {
            let Some((phys, size)) = route.bar5_info() else {
                continue;
            };

            let owner = format!("{}", route.address);
            self.map(mmu, dma, &owner, phys, size);
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
