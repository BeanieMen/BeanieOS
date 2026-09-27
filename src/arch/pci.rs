use alloc::vec::Vec;
use pci_types::{Bar, CommandRegister, ConfigRegionAccess, EndpointHeader, PciAddress, PciHeader};
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

struct PciConfig;

impl ConfigRegionAccess for PciConfig {
    unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
        let config_address = 0x8000_0000
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);

        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);

        unsafe {
            address_port.write(config_address);
            data_port.read()
        }
    }

    unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
        let config_address = 0x8000_0000
            | ((address.bus() as u32) << 16)
            | ((address.device() as u32) << 11)
            | ((address.function() as u32) << 8)
            | ((offset as u32) & 0xFC);

        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);

        unsafe {
            address_port.write(config_address);
            data_port.write(value);
        }
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

impl Device {
    pub fn bar5_info(&self) -> Option<(u64, u64)> {
        match self.bar(5)? {
            Bar::Memory32 { address, size, .. } => Some((address as u64, size as u64)),
            Bar::Memory64 { address, size, .. } => Some((address, size)),
            _ => None,
        }
    }
    pub fn is_ahci(&self) -> bool {
        self.class == 0x01 && self.subclass == 0x06 && self.interface == 0x01
    }

    pub fn bar(&self, index: u8) -> Option<Bar> {
        let config = PciConfig;
        let header = PciHeader::new(self.address);

        let endpoint = EndpointHeader::from_header(header, &config)?;

        endpoint.bar(index, &config)
    }

    pub fn ahci_base(&self) -> Option<usize> {
        if !self.is_ahci() {
            return None;
        }

        let (base, _) = self.bar5_info()?;

        if base == 0 || base >= 8 * 1024 * 1024 * 1024 {
            return None;
        }

        Some(base as usize)
    }
    pub fn enable(&self) {
        let config = PciConfig;
        let mut header = PciHeader::new(self.address);

        header.update_command(&config, |mut command| {
            command.insert(CommandRegister::MEMORY_ENABLE | CommandRegister::BUS_MASTER_ENABLE);
            command
        });
    }
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

                let (_, class, subclass, interface) = header.revision_and_class(&config);

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

pub fn find_ahci() -> Vec<Device> {
    scan().into_iter().filter(Device::is_ahci).collect()
}
