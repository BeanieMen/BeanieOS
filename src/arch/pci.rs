use alloc::vec::Vec;
use pci_types::{ConfigRegionAccess, PciAddress, PciHeader};
use x86_64::instructions::port::Port;

// ports
const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

struct PciConfig;

impl ConfigRegionAccess for PciConfig {
    unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
        let config_address =
            0x8000_0000
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);

        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);

        address_port.write(config_address);
        data_port.read()
    }

    unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
        let config_address =
            0x8000_0000
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);

        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);

        address_port.write(config_address);
        data_port.write(value);
    }
}

#[derive(Clone, Copy)]
pub struct Device {
    pub address: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub interface: u8,
}

pub fn scan() -> Vec<Device> {
    let config = PciConfig;
    let mut devices = Vec::new();

    for bus in 0..=255 {
        for device in 0..32 {
            for function in 0..8 {
                let address = PciAddress::new(0, bus, device, function);
                let header = PciHeader::new(address);

                let (vendor_id, device_id) = header.id(&config);

                if vendor_id == 0xFFFF {
                    continue;
                }

                let (_, class, subclass, interface) =
                    header.revision_and_class(&config);

                devices.push(Device {
                    address,
                    vendor_id,
                    device_id,
                    class,
                    subclass,
                    interface,
                });
            }
        }
    }

    devices
}