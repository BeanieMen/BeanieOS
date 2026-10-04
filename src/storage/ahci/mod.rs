pub mod controller;
mod disk;
pub mod irq;
pub mod partition;
mod regs;

use alloc::vec::Vec;

pub use controller::AhciController;
pub use disk::Disk;
pub use irq::on_interrupt;

use crate::kwarn;

pub fn find_disks() -> Vec<Disk> {
    let mut disks = Vec::new();

    for device in crate::hal::hal().pci.find_ahci() {
        match AhciController::new(&device) {
            Ok(mut controller) => disks.extend(controller.find_disks()),
            Err(why) => kwarn!("controller {}: {why}", device.address),
        }
    }

    disks
}
