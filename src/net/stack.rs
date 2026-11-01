use alloc::vec::Vec;
use crate::arch::lock::InterruptMutex;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address, Ipv4Cidr};

use super::e1000::E1000_DEVICE;
use super::raw;

pub struct SmolDevice;

impl Device for SmolDevice {
    type RxToken<'a> = SmolRxToken;
    type TxToken<'a> = SmolTxToken;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let dev = E1000_DEVICE.lock();
        let packet = dev.as_ref()?.receive()?;
        // Tap a copy for raw (AF_PACKET-style) sockets before the stack
        // consumes the frame, so userspace network clients see everything.
        raw::tap_frame(&packet);
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

pub static NET_STATE: InterruptMutex<Option<NetState>> = InterruptMutex::new(None);

/// Bring up the NIC and the TCP/IP stack with NO address configuration.
/// Choosing an address is policy and belongs to userspace (e.g. /bin/dhcpc),
/// which applies it through the interface-configuration ioctls below.
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

    let iface = Interface::new(config, &mut device, Instant::from_millis(0));
    let sockets = SocketSet::new(alloc::vec![]);

    *NET_STATE.lock() = Some(NetState { iface, sockets });
}

pub fn poll() {
    let ms = crate::arch::interrupts::vectors::ticks();
    poll_at(Instant::from_millis(ms as i64));
}

fn poll_at(instant: Instant) {
    let Some(mut guard) = NET_STATE.try_lock() else {
        return;
    };
    let Some(state) = guard.as_mut() else {
        return;
    };
    let mut device = SmolDevice;
    state.iface.poll(instant, &mut device, &mut state.sockets);
}

/// MAC of the NIC backing the stack, for userspace network clients.
pub fn mac_address() -> Option<[u8; 6]> {
    E1000_DEVICE.lock().as_ref().map(|d| d.mac())
}

/// Apply an IPv4 address/prefix chosen by userspace. Replaces any previous
/// address. Pure mechanism: no policy about which address is right.
pub fn set_ipv4_address(addr: [u8; 4], mask: [u8; 4]) -> Result<(), &'static str> {
    let mask_bits = u32::from_be_bytes(mask);
    let prefix = mask_bits.count_ones() as u8;
    // A mask must be a contiguous run of ones followed by zeros.
    if prefix == 0 || mask_bits != u32::MAX << (32 - prefix) {
        return Err("invalid mask");
    }
    let cidr = Ipv4Cidr::new(
        Ipv4Address::new(addr[0], addr[1], addr[2], addr[3]),
        prefix,
    );
    let mut guard = NET_STATE.lock();
    let state = guard.as_mut().ok_or("network stack not initialized")?;
    state.iface.update_ip_addrs(|addrs| {
        addrs.clear();
        let _ = addrs.push(IpCidr::Ipv4(cidr));
    });
    Ok(())
}

/// Apply or remove the userspace-chosen default gateway. All-zero removes
/// the default route.
pub fn set_default_gateway(gw: [u8; 4]) -> Result<(), &'static str> {
    let mut guard = NET_STATE.lock();
    let state = guard.as_mut().ok_or("network stack not initialized")?;
    if gw == [0, 0, 0, 0] {
        let _ = state.iface.routes_mut().remove_default_ipv4_route();
    } else {
        state
            .iface
            .routes_mut()
            .add_default_ipv4_route(Ipv4Address::new(gw[0], gw[1], gw[2], gw[3]))
            .map_err(|_| "route table full")?;
    }
    Ok(())
}
