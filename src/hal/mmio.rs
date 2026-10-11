use alloc::{format, vec::Vec};

use x86_64::structures::paging::{PageTableFlags, PhysFrame};

use super::dma::Dma;
use crate::arch::lock::InterruptMutex;
use crate::kdebug;
use crate::memory::mmu::{KERNEL_RW, Mapper, MemoryError};
use crate::memory::pool::Area;
use crate::paging::{MMIO_BASE, MMIO_WINDOW, PAGE_SIZE};

const MMIO_FLAGS: PageTableFlags = PageTableFlags::from_bits_truncate(
    KERNEL_RW.bits() | PageTableFlags::NO_CACHE.bits() | PageTableFlags::WRITE_THROUGH.bits(),
);

#[derive(Clone)]
pub(crate) struct Mapping {
    pub va: usize,
    pub phys: u64,
    pub size: u64,
}

pub(crate) struct Mmapped {
    mappings: InterruptMutex<Vec<Mapping>>,
    bump: InterruptMutex<u64>,
}

fn page_span(phys: u64, size: u64) -> (u64, u64) {
    let start = phys & !(PAGE_SIZE - 1);
    let end = (phys + size.max(PAGE_SIZE) + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    (start, end)
}

impl Mmapped {
    pub(crate) const fn new() -> Self {
        Mmapped {
            mappings: InterruptMutex::new(Vec::new()),
            bump: InterruptMutex::new(MMIO_BASE),
        }
    }

    pub(crate) fn mapping_for(&self, phys: u64) -> Option<Mapping> {
        self.mappings
            .lock()
            .iter()
            .find(|m| phys >= m.phys && phys < m.phys + m.size)
            .cloned()
    }

    pub(crate) fn map(
        &self,
        mapper: &Mapper,
        dma: &Dma,
        owner: &str,
        phys: u64,
        size: u64,
    ) -> Option<usize> {
        let (start, end) = page_span(phys, size);
        let len = end - start;

        let va = {
            let mut bump = self.bump.lock();
            let va = (*bump + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

            if va - MMIO_BASE + len > MMIO_WINDOW {
                return None;
            }

            *bump = va + len;
            va
        };

        map_range(mapper, va, start, len, MMIO_FLAGS).ok()?;

        dma.reserve(Area::new(start, end));

        self.mappings.lock().push(Mapping {
            va: va as usize,
            phys: start,
            size: len,
        });

        kdebug!("  mmio {owner} {start:#x}..{end:#x} at {va:#x}");

        Some(va as usize)
    }

    pub(crate) fn map_all_bars(
        &self,
        mapper: &Mapper,
        dma: &Dma,
        routes: &crate::hal::pci::Routes,
    ) {
        for route in routes.iter() {
            let Some((phys, size)) = route.bar5_info() else {
                continue;
            };

            let owner = format!("{}", route.address);
            self.map(mapper, dma, &owner, phys, size);
        }
    }

    pub(crate) fn reserve_all_bars(&self, dma: &Dma, routes: &crate::hal::pci::Routes) {
        for route in routes.iter() {
            let Some((phys, size)) = route.bar5_info() else {
                continue;
            };

            let (start, end) = page_span(phys, size);

            kdebug!("  reserve mmio {} {start:#x}..{end:#x}", route.address);
            dma.reserve(Area::new(start, end));
        }
    }
}

fn map_range(
    mapper: &Mapper,
    virt: u64,
    phys: u64,
    size: u64,
    flags: PageTableFlags,
) -> Result<(), MemoryError> {
    let pages = size.div_ceil(PAGE_SIZE);

    for i in 0..pages {
        let v = (virt + i * PAGE_SIZE) & !(PAGE_SIZE - 1);
        let p = (phys + i * PAGE_SIZE) & !(PAGE_SIZE - 1);

        mapper.map(v, PhysFrame::containing_address(x86_64::PhysAddr::new(p)), flags)?;
    }

    Ok(())
}

impl Default for Mmapped {
    fn default() -> Self {
        Self::new()
    }
}