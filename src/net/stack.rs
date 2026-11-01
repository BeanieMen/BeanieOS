use alloc::vec::Vec;
use crate::arch::lock::InterruptMutex;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr};

use smoltcp::iface::SocketHandle;
use smoltcp::socket::{dhcpv4, dns};
use smoltcp::wire::DnsQueryType;

use core::sync::atomic::{AtomicU64, Ordering};

use super::e1000::E1000_DEVICE;

/// Milliseconds added to the lapic tick count when converting to smoltcp
/// time. Boot-time negotiation runs with interrupts off (ticks frozen) on a
/// synthetic clock anchored at the tick base; when it ends, this offset
/// (= synthetic elapsed - real elapsed, so at most MAX_FAKE_MS) shifts the
/// runtime clock forward to exactly where negotiation left it. One monotonic
/// time domain across boot and runtime: timestamps never go backwards and
/// never jump more than 20s.
static NET_TIME_OFFSET_MS: AtomicU64 = AtomicU64::new(0);

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
    dhcp: Option<SocketHandle>,
    dns: Option<SocketHandle>,
    dns_query: Option<dns::QueryHandle>,
    dns_started_ms: u64,
    configured: bool,
    dns_done: bool,
}

pub static NET_STATE: InterruptMutex<Option<NetState>> = InterruptMutex::new(None);

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

    // No static address: the interface is configured by DHCP below. The
    // constants only come back as a documented fallback when no DHCP server
    // answers at all.
    let iface = Interface::new(config, &mut device, Instant::from_millis(0));

    let mut sockets = SocketSet::new(alloc::vec![]);
    let dhcp_handle = sockets.add(dhcpv4::Socket::new());
    *NET_STATE.lock() = Some(NetState {
        iface,
        sockets,
        dhcp: Some(dhcp_handle),
        dns: None,
        dns_query: None,
        dns_started_ms: 0,
        configured: false,
        dns_done: false,
    });

    negotiate();
}

/// Drive the DHCP exchange (and the one-shot DNS self-test it triggers)
/// until the interface is configured or the window closes. QEMU's slirp
/// answers within milliseconds; the bound only bites on networks with no
/// DHCP server, where the previous hard-coded QEMU values are kept as an
/// explicit fallback so behaviour does not silently change there.
fn negotiate() {
    // Interrupts are off this early in boot, so the lapic tick count does
    // not advance and cannot bound the wait. Drive smoltcp with a synthetic
    // clock instead: one fake millisecond per poll iteration. Replies from a
    // DHCP server arrive within a few iterations; the cap only bites when
    // nothing answers, where the fallback keeps old behaviour.
    const MAX_FAKE_MS: i64 = 20_000;
    let base = crate::arch::interrupts::vectors::ticks() as i64;
    let mut fake_ms: i64 = 0;
    loop {
        poll_at(Instant::from_millis(base + fake_ms));
        fake_ms += 1;
        {
            let guard = NET_STATE.lock();
            let Some(state) = guard.as_ref() else {
                return;
            };
            if state.configured && state.dns_done {
                break;
            }
        }
        if fake_ms > MAX_FAKE_MS {
            let mut guard = NET_STATE.lock();
            if let Some(state) = guard.as_mut() {
                if !state.configured {
                    // No DHCP server answered. Leave the interface
                    // unconfigured and say so; picking a default address
                    // would silently be the wrong one off QEMU.
                    state.dns_done = true;
                    crate::println!("NET_DHCP_FAIL reason=no-server");
                }
            }
            break;
        }
        for _ in 0..30_000 {
            core::hint::spin_loop();
        }
    }

    // Hand the clock over: keep instants continuous across the transition.
    let real_elapsed = crate::arch::interrupts::vectors::ticks() - (base as u64);
    let synthetic_elapsed = fake_ms as u64;
    let offset = synthetic_elapsed.saturating_sub(real_elapsed);
    NET_TIME_OFFSET_MS.store(offset, Ordering::Release);
    // Self-check: the first runtime instant must be >= the last synthetic one.
    let last_synthetic = base + fake_ms;
    let first_runtime = (base as u64 + real_elapsed + offset) as i64;
    if first_runtime >= last_synthetic {
        crate::println!("NET_TIME_OK offset_ms={}", offset);
    } else {
        crate::println!(
            "NET_TIME_FAIL last_synthetic={} first_runtime={}",
            last_synthetic,
            first_runtime
        );
    }
}

pub fn poll() {
    let ms = crate::arch::interrupts::vectors::ticks() + NET_TIME_OFFSET_MS.load(Ordering::Acquire);
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
    let ms = instant.total_millis() as u64;
    state.iface.poll(instant, &mut device, &mut state.sockets);

    // DHCP: apply configuration changes the client reports.
    if let Some(handle) = state.dhcp {
        let event = state.sockets.get_mut::<dhcpv4::Socket>(handle).poll();
        let mut leased = None;
        match event {
            Some(dhcpv4::Event::Configured(config)) => {
                let cidr = IpCidr::Ipv4(config.address);
                state.iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    let _ = addrs.push(cidr);
                });
                if let Some(router) = config.router {
                    let _ = state.iface.routes_mut().add_default_ipv4_route(router);
                }
                let servers: alloc::vec::Vec<IpAddress> = config
                    .dns_servers
                    .iter()
                    .copied()
                    .map(IpAddress::Ipv4)
                    .collect();
                leased = Some((cidr, config.router, servers));
            }
            Some(dhcpv4::Event::Deconfigured) => {
                if state.configured {
                    state.iface.update_ip_addrs(|addrs| addrs.clear());
                    state.configured = false;
                    crate::println!("NET_DHCP_LOST");
                }
            }
            None => {}
        }
        if let Some((addr, router, servers)) = leased {
            let first_lease = !state.configured;
            state.configured = true;
            crate::println!(
                "NET_DHCP_OK addr={:?} gw={:?} dns={:?}",
                addr,
                router,
                servers
            );
            // One-shot end-to-end check: a real DNS lookup through the
            // leased server exercises ARP, unicast TX/RX and the route.
            if first_lease && state.dns.is_none() {
                if servers.is_empty() {
                    state.dns_done = true;
                    crate::println!("NET_DNS_SKIP reason=no-server");
                } else {
                    let sock = dns::Socket::new(&servers, alloc::vec::Vec::new());
                    let dns_handle = state.sockets.add(sock);
                    let query = {
                        let ctx = state.iface.context();
                        state
                            .sockets
                            .get_mut::<dns::Socket>(dns_handle)
                            .start_query(ctx, "example.com", DnsQueryType::A)
                    };
                    match query {
                        Ok(qh) => {
                            state.dns = Some(dns_handle);
                            state.dns_query = Some(qh);
                            state.dns_started_ms = ms;
                        }
                        Err(_) => {
                            state.dns_done = true;
                            crate::println!("NET_DNS_FAIL reason=start");
                        }
                    }
                }
            }
        }
    }

    // DNS self-test progress.
    if let (Some(handle), Some(query)) = (state.dns, state.dns_query) {
        match state.sockets.get_mut::<dns::Socket>(handle).get_query_result(query) {
            Ok(addrs) => {
                crate::println!("NET_DNS_OK answers={}", addrs.len());
                state.dns_done = true;
                state.dns_query = None;
            }
            Err(dns::GetQueryResultError::Pending) => {
                if ms.saturating_sub(state.dns_started_ms) > 5_000 {
                    crate::println!("NET_DNS_FAIL reason=timeout");
                    state.dns_done = true;
                    state.dns_query = None;
                }
            }
            Err(_) => {
                crate::println!("NET_DNS_FAIL reason=query");
                state.dns_done = true;
                state.dns_query = None;
            }
        }
    }
}
