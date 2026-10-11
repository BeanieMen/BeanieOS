pub mod controller;
pub mod disk;
pub mod irq;
mod regs;

use alloc::vec::Vec;

pub(crate) use controller::AhciController;
pub(crate) use disk::disk::Disk;
pub(crate) use irq::on_interrupt;

use crate::kwarn;

pub(crate) fn find_disks() -> Vec<Disk> {
    let mut disks = Vec::new();

    for device in crate::hal::hal().pci.find_ahci() {
        match AhciController::new(&device) {
            Ok(mut controller) => disks.extend(controller.find_disks()),
            Err(why) => kwarn!("controller {}: {why}", device.address),
        }
    }

    disks
}
