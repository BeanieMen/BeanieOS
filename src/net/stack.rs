use alloc::vec::Vec;
use spin::Mutex;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address};

use super::e1000::E1000_DEVICE;

pub struct SmolDevice;

impl Device for SmolDevice {
    type RxToken<'a> = SmolRxToken;
    type TxToken<'a> = SmolTxToken;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let dev = E1000_DEVICE.lock();
        let packet = dev.as_ref()?.receive()?;
        Some((SmolRxToken(packet), SmolTxToken))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(SmolTxToken)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        caps
    }
}

pub struct SmolRxToken(Vec<u8>);

impl RxToken for SmolRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}

pub struct SmolTxToken;

impl TxToken for SmolTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = alloc::vec![0u8; len];
        let res = f(&mut buf);
        if let Some(dev) = E1000_DEVICE.lock().as_ref() {
            dev.transmit(&buf);
        }
        res
    }
}

pub struct NetState {
    pub iface: Interface,
    pub sockets: SocketSet<'static>,
}

pub static NET_STATE: Mutex<Option<NetState>> = Mutex::new(None);

pub fn init_stack() {
    let mac = {
        let dev = E1000_DEVICE.lock();
        let Some(d) = dev.as_ref() else {
            return;
        };
        d.mac()
    };

    let mut device = SmolDevice;
    let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));
    let mut config = Config::new(hw_addr);
    config.random_seed = 0x12345678;

    let mut iface = Interface::new(config, &mut device, Instant::from_millis(0));
    iface.update_ip_addrs(|addrs| {
        let _ = addrs.push(IpCidr::new(
            IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 15)),
            24,
        ));
    });
    iface
        .routes_mut()
        .add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2))
        .unwrap();

    let sockets = SocketSet::new(alloc::vec![]);
    *NET_STATE.lock() = Some(NetState { iface, sockets });
}

pub fn poll() {
    let mut guard = NET_STATE.lock();
    let Some(state) = guard.as_mut() else {
        return;
    };
    let mut device = SmolDevice;
    let ms = crate::arch::interrupts::vectors::ticks();
    let instant = Instant::from_millis(ms as i64);
    state.iface.poll(instant, &mut device, &mut state.sockets);
}
