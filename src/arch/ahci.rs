use simple_ahci::AhciDriver;

use crate::{arch::pci::Device, println};

pub struct Hal;

impl simple_ahci::Hal for Hal {
    fn virt_to_phys(va: usize) -> usize {
        va
    }

    fn current_ms() -> u64 {
        unsafe {
            core::arch::x86_64::_rdtsc() / 3_000_000
        }
    }

    fn flush_dcache() {}
}

pub type Ahci = AhciDriver<Hal>;

pub fn init(device: &Device) -> Option<Ahci> {
    let abar = device.ahci_base()?;

    println!("AHCI controller found");
    println!("ABAR: {:#x}", abar);

    let abar = abar.try_into().ok()?;

    println!("creating AHCI driver");

    let driver = unsafe { Ahci::try_new(abar) };

    println!("AHCI driver created");

    driver
}