use alloc::vec::Vec;

use super::disk::disk::Disk;
use super::regs::*;
use crate::hal::dma::{AHCI_DMA, AhciDma};
use crate::hal::pci::Device;
use crate::{kinfo, kwarn};

/// the spec allows 1 ms for GHC.HR to set, the emulator is slower.
const HR_SET_MS: u64 = 1_000;

pub(crate) struct AhciController {
    dma: &'static AhciDma,
    abar: usize,
    ports: u32,
}

impl AhciController {
    pub(crate) fn new(device: &Device) -> Result<Self, &'static str> {
        let (phys, _) = device.bar5_info().ok_or("no BAR5")?;
        let abar = crate::hal::hal()
            .mmio
            .mapping_for(phys)
            .map(|m| m.va)
            .ok_or("BAR5 is not mapped")?;

        device.enable();

        if device.interrupt_disabled() {
            device.unmask_interrupt();
            kinfo!("  INTx was masked in the command register, unmapped");
        }

        let capabilities = reg(abar, CAP);
        let version = reg(abar, VS);
        let implemented = reg(abar, PI);

        let wide = capabilities & 1 != 0;
        let major = version >> 16;
        let minor = version & 0xffff;

        kinfo!(
            "AHCI at {abar:#x} v{major}.{minor}, ports {implemented:#b}, 64 bit {}",
            if wide { "yes" } else { "no" }
        );

        set_reg(abar, GHC, GHC_HR);
        if !wait_for(HR_SET_MS, || reg(abar, GHC) & GHC_HR == 0) {
            return Err("HBA reset did not complete");
        }

        set_reg(abar, GHC, GHC_AE);

        AHCI_DMA.clear();

        let pin = device.interrupt_pin().ok_or("no PCI header")?;
        if pin == 0 {
            return Err("no interrupt pin assigned");
        }

        let line = device.interrupt_line().ok_or("no interrupt line routed")?;
        super::irq::attach(abar, implemented, pin, line);

        Ok(Self {
            dma: &AHCI_DMA,
            abar,
            ports: implemented,
        })
    }

    pub(crate) fn find_disks(&mut self) -> Vec<Disk> {
        let mut disks = Vec::new();

        for port in 0..32 {
            if self.ports & (1 << port) == 0 {
                continue;
            }

            match Disk::attach(self.dma, self.abar, port) {
                Ok(disk) => disks.push(disk),
                Err(why) => kwarn!("  port {port}: {why}"),
            }
        }

        disks
    }
}
