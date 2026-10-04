pub mod dma;
pub mod mmio;
pub mod pci;

pub use dma::Region;

use spin::Once;

use crate::memory::mmu::MMU;

pub static HAL: Once<Hal> = Once::new();

pub struct Hal {
    pub pci: pci::Pci,
    pub mmio: mmio::Mmapped,
    pub dma: dma::Dma,
}

impl Hal {
    const fn new() -> Self {
        Hal {
            pci: pci::Pci::new(),
            mmio: mmio::Mmapped::new(),
            dma: dma::Dma::new(),
        }
    }
}

pub fn hal() -> &'static Hal {
    HAL.call_once(Hal::new)
}

pub fn discover() -> pci::Routes {
    let hal = hal();

    let routes = hal.pci.ahci_bars();
    hal.mmio.reserve_all_bars(&hal.dma, &routes);

    routes
}

pub fn init(mmu: &mut MMU<'_>, routes: &pci::Routes) {
    let hal = hal();

    hal.mmio.map_all_bars(mmu, &hal.dma, routes);
}
