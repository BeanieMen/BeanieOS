use alloc::vec::Vec;

use super::Disk;
use crate::hal::dma::{AHCI_DMA, AhciDma};
use super::regs::*;
use crate::hal::pci::Device;
use crate::{kdebug, kinfo};

pub struct AhciController {
    dma: &'static AhciDma,
    abar: usize,
    ports: u32,
}

impl AhciController {
    pub fn new(device: &Device) -> Result<Self, &'static str> {
        let (phys, _) = device.bar5_info().ok_or("no BAR5")?;
        let abar = crate::hal::hal()
            .mmio
            .mapping_at(crate::hal::mmio::MMIO_BASE as usize + phys as usize)
            .map(|m| m.va)
            .unwrap_or(crate::hal::mmio::MMIO_BASE as usize + phys as usize);

        // Command bits 1 (memory) and 2 (bus master). Sizing a BAR writes it,
        // so this follows any probing.
        device.enable();

        // Firmware masks INTx when handing the line over; this driver owns the
        // line now, so unmask it before anything asserts into a mask.
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
        if !wait_for(1000, || reg(abar, GHC) & GHC_HR == 0) {
            return Err("HBA reset did not complete");
        }

        // GHC.AE (bit 1): the HBA does nothing at all without it.
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

    pub fn find_disks(&mut self) -> Vec<Disk> {
        let mut disks = Vec::new();

        for port in 0..32 {
            if self.ports & (1 << port) == 0 {
                continue;
            }

            match Disk::attach(self.dma, self.abar, port) {
                Ok(disk) => disks.push(disk),
                Err(why) => kdebug!("  port {port}: {why}"),
            }
        }

        disks
    }
}
