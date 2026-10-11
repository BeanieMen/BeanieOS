use alloc::vec::Vec;

use pci_types::{Bar, CommandRegister, ConfigRegionAccess, EndpointHeader, PciAddress, PciHeader};
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

const MAX_ROUTES: usize = 32;

struct PciConfig;

impl ConfigRegionAccess for PciConfig {
    unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);

        unsafe {
            address_port.write(config_address(address, offset));
            data_port.read()
        }
    }

    unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);

        unsafe {
            address_port.write(config_address(address, offset));
            data_port.write(value);
        }
    }
}

fn config_address(address: PciAddress, offset: u16) -> u32 {
    0x8000_0000
        | ((address.bus() as u32) << 16)
        | ((address.device() as u32) << 11)
        | ((address.function() as u32) << 8)
        | ((offset as u32) & 0xFC)
}

#[derive(Clone, Copy)]
pub(crate) struct Device {
    pub address: PciAddress,
    #[allow(dead_code)]
    pub vendor_id: u16,
    #[allow(dead_code)]
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub interface: u8,
}

impl Device {
    pub(crate) fn bar(&self, index: u8) -> Option<Bar> {
        let config = PciConfig;
        let header = PciHeader::new(self.address);

        let endpoint = EndpointHeader::from_header(header, &config)?;

        endpoint.bar(index, &config)
    }

    fn bar_info(&self, index: u8) -> Option<(u64, u64)> {
        match self.bar(index)? {
            Bar::Memory32 { address, size, .. } => Some((address as u64, size as u64)),
            Bar::Memory64 { address, size, .. } => Some((address, size)),
            _ => None,
        }
    }

    pub(crate) fn bar5_info(&self) -> Option<(u64, u64)> {
        self.bar_info(5)
    }

    fn is_ahci(&self) -> bool {
        self.class == 0x01 && self.subclass == 0x06 && self.interface == 0x01
    }

    // INTx line firmware assigned: 1 is INTA# through 4 for INTD#, 0 unconnected.
    pub(crate) fn interrupt_pin(&self) -> Option<u8> {
        let config = PciConfig;
        let header = PciHeader::new(self.address);

        let endpoint = EndpointHeader::from_header(header, &config)?;

        Some(endpoint.interrupt(&config).0)
    }

    // GSI INTx was routed to. 0 or 0xff is unassigned. Not the pin: pin 1 is INTA#.
    pub(crate) fn interrupt_line(&self) -> Option<u8> {
        let config = PciConfig;
        let header = PciHeader::new(self.address);

        let endpoint = EndpointHeader::from_header(header, &config)?;

        let line = endpoint.interrupt(&config).1;

        ((line != 0) && (line != 0xff)).then_some(line)
    }

    // Command register bit 10. While set the device asserts nothing.
    pub(crate) fn interrupt_disabled(&self) -> bool {
        let config = PciConfig;
        let header = PciHeader::new(self.address);

        header
            .command(&config)
            .contains(CommandRegister::INTERRUPT_DISABLE)
    }

    pub(crate) fn unmask_interrupt(&self) {
        let config = PciConfig;
        let mut header = PciHeader::new(self.address);

        header.update_command(&config, |mut command| {
            command.remove(CommandRegister::INTERRUPT_DISABLE);
            command
        });
    }

    pub(crate) fn enable(&self) {
        let config = PciConfig;
        let mut header = PciHeader::new(self.address);

        header.update_command(&config, |mut command| {
            command.insert(CommandRegister::MEMORY_ENABLE | CommandRegister::BUS_MASTER_ENABLE);
            command
        });
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Route {
    pub address: PciAddress,
}

impl Route {
    pub(crate) fn bar5_info(&self) -> Option<(u64, u64)> {
        let config = PciConfig;
        let header = PciHeader::new(self.address);

        let endpoint = EndpointHeader::from_header(header, &config)?;

        match endpoint.bar(5, &config)? {
            Bar::Memory32 { address, size, .. } => Some((address as u64, size as u64)),
            Bar::Memory64 { address, size, .. } => Some((address, size)),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Routes {
    entries: [Option<Route>; MAX_ROUTES],
    count: usize,
}

impl Routes {
    pub(crate) const fn new() -> Self {
        Routes {
            entries: [None; MAX_ROUTES],
            count: 0,
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Route> + '_ {
        self.entries.iter().take(self.count).flatten().copied()
    }

    fn push(&mut self, route: Route) {
        if self.count >= MAX_ROUTES {
            return;
        }

        self.entries[self.count] = Some(route);
        self.count += 1;
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }
}

pub(crate) struct Pci;

impl Pci {
    pub(crate) const fn new() -> Self {
        Pci
    }

    pub(crate) fn ahci_bars(&self) -> Routes {
        let config = PciConfig;
        let mut routes = Routes::new();

        for bus in 0..=255 {
            for device in 0..32 {
                for function in 0..8 {
                    let address = PciAddress::new(0, bus, device, function);
                    let header = PciHeader::new(address);

                    let (vendor_id, _) = header.id(&config);

                    if vendor_id == 0xFFFF {
                        continue;
                    }

                    let (_, class, subclass, interface) = header.revision_and_class(&config);

                    if class != 0x01 || subclass != 0x06 || interface != 0x01 {
                        continue;
                    }

                    routes.push(Route { address });
                }
            }
        }

        routes
    }

    pub(crate) fn scan(&self) -> Vec<Device> {
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

    pub(crate) fn find_ahci(&self) -> Vec<Device> {
        self.scan().into_iter().filter(Device::is_ahci).collect()
    }
}

impl Default for Pci {
    fn default() -> Self {
        Self::new()
    }
}
