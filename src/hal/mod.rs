pub mod dma;
pub mod mmio;
pub mod pci;

use spin::Once;

use crate::arch::lock::InterruptMutex;

use crate::memory::mmu::MMU;

pub(crate) use dma::Dma;
pub(crate) use mmio::Mmapped;
pub(crate) use pci::{Pci, Routes};

static HAL: Once<Hal> = Once::new();

pub(crate) struct Hal {
    pub pci: Pci,
    pub mmio: Mmapped,
    pub dma: Dma,
    routes: InterruptMutex<Routes>,
}

impl Hal {
    const fn new() -> Self {
        Hal {
            pci: Pci::new(),
            mmio: Mmapped::new(),
            dma: Dma::new(),
            routes: InterruptMutex::new(Routes::new()),
        }
    }
}

pub(crate) fn hal() -> &'static Hal {
    HAL.call_once(Hal::new)
}

pub(crate) fn routes() -> Routes {
    hal().routes.lock().clone()
}

pub(crate) fn init(mmu: &mut MMU<'_>) {
    let hal = hal();
    let found = hal.pci.ahci_bars();

    hal.mmio.reserve_all_bars(&hal.dma, &found);

    hal.mmio.map_all_bars(mmu, &hal.dma, &found);
    *hal.routes.lock() = found;
}
