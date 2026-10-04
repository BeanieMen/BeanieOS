pub mod dma;
pub mod mmio;
pub mod pci;

use spin::{Mutex, Once};

use crate::memory::allocator::RESERVED;
use crate::memory::mmu::MMU;

pub use dma::{Dma, Region};
pub use mmio::Mmapped;
pub use pci::{Pci, Routes};

pub static HAL: Once<Hal> = Once::new();

pub struct Hal {
    pub pci: Pci,
    pub mmio: Mmapped,
    pub dma: Dma,
    routes: Mutex<Routes>,
}

impl Hal {
    const fn new() -> Self {
        Hal {
            pci: Pci::new(),
            mmio: Mmapped::new(),
            dma: Dma::new(),
            routes: Mutex::new(Routes::new()),
        }
    }
}

pub fn hal() -> &'static Hal {
    HAL.call_once(Hal::new)
}

pub fn routes() -> Routes {
    hal().routes.lock().clone()
}

pub fn init(mmu: &mut MMU<'_>) {
    let hal = hal();
    let found = hal.pci.ahci_bars();

    hal.mmio.reserve_all_bars(&hal.dma, &found);
    RESERVED.snapshot();

    hal.mmio.map_all_bars(mmu, &hal.dma, &found);
    *hal.routes.lock() = found;
}