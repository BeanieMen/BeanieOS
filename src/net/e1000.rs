use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::arch::lock::InterruptMutex;
use x86_64::VirtAddr;

use crate::memory::allocator::alloc_frame;
use crate::paging::phys_to_direct;

const REG_CTRL: usize = 0x0000;
const REG_STATUS: usize = 0x0008;
const REG_EEPROM: usize = 0x0014;
const REG_ICR: usize = 0x00C0;
const REG_IMS: usize = 0x00D0;
const REG_RCTL: usize = 0x0100;
const REG_RDBAL: usize = 0x2800;
const REG_RDBAH: usize = 0x2804;
const REG_RDLEN: usize = 0x2808;
const REG_RDH: usize = 0x2810;
const REG_RDT: usize = 0x2818;
const REG_TCTL: usize = 0x0400;
const REG_TDBAL: usize = 0x3800;
const REG_TDBAH: usize = 0x3804;
const REG_TDLEN: usize = 0x3808;
const REG_TDH: usize = 0x3810;
const REG_TDT: usize = 0x3818;
const REG_RAL: usize = 0x5400;
const REG_RAH: usize = 0x5404;

const NUM_RX_DESC: usize = 32;
const NUM_TX_DESC: usize = 32;
const BUF_SIZE: usize = 2048;

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

pub struct E1000 {
    mmio: VirtAddr,
    mac: [u8; 6],
    rx_cur: AtomicUsize,
    tx_cur: AtomicUsize,
    rx_buffers: Vec<u64>,
    tx_buffers: Vec<u64>,
    rx_desc_virt: u64,
    tx_desc_virt: u64,
}

unsafe impl Send for E1000 {}
unsafe impl Sync for E1000 {}

pub static E1000_DEVICE: InterruptMutex<Option<E1000>> = InterruptMutex::new(None);

impl E1000 {
    unsafe fn write_reg(&self, reg: usize, val: u32) {
        unsafe {
            ((self.mmio.as_u64() + reg as u64) as *mut u32).write_volatile(val);
        }
    }

    unsafe fn read_reg(&self, reg: usize) -> u32 {
        unsafe { ((self.mmio.as_u64() + reg as u64) as *const u32).read_volatile() }
    }

    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    pub fn init(mmio: VirtAddr) -> Self {
        let mut dev = E1000 {
            mmio,
            mac: [0; 6],
            rx_cur: AtomicUsize::new(0),
            tx_cur: AtomicUsize::new(0),
            rx_buffers: Vec::with_capacity(NUM_RX_DESC),
            tx_buffers: Vec::with_capacity(NUM_TX_DESC),
            rx_desc_virt: 0,
            tx_desc_virt: 0,
        };

        unsafe {
            let ral = dev.read_reg(REG_RAL);
            let rah = dev.read_reg(REG_RAH);
            if rah & (1 << 31) != 0 {
                dev.mac[0] = (ral & 0xff) as u8;
                dev.mac[1] = ((ral >> 8) & 0xff) as u8;
                dev.mac[2] = ((ral >> 16) & 0xff) as u8;
                dev.mac[3] = ((ral >> 24) & 0xff) as u8;
                dev.mac[4] = (rah & 0xff) as u8;
                dev.mac[5] = ((rah >> 8) & 0xff) as u8;
            } else {
                dev.mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
            }

            let rx_desc_frame = alloc_frame().expect("e1000 rx desc frame").start_address().as_u64();
            let rx_desc_virt = phys_to_direct(rx_desc_frame);
            dev.rx_desc_virt = rx_desc_virt;
            let rx_descs =
                core::slice::from_raw_parts_mut(rx_desc_virt as *mut RxDesc, NUM_RX_DESC);

            for desc in rx_descs.iter_mut() {
                let buf_frame = alloc_frame().expect("e1000 rx buf frame").start_address().as_u64();
                dev.rx_buffers.push(buf_frame);
                desc.addr = buf_frame;
                desc.status = 0;
            }

            dev.write_reg(REG_RDBAL, rx_desc_frame as u32);
            dev.write_reg(REG_RDBAH, (rx_desc_frame >> 32) as u32);
            dev.write_reg(
                REG_RDLEN,
                (NUM_RX_DESC * core::mem::size_of::<RxDesc>()) as u32,
            );
            dev.write_reg(REG_RDH, 0);
            dev.write_reg(REG_RDT, (NUM_RX_DESC - 1) as u32);
            dev.write_reg(
                REG_RCTL,
                (1 << 1) | (1 << 2) | (1 << 4) | (1 << 15) | (1 << 26),
            );

            let tx_desc_frame = alloc_frame().expect("e1000 tx desc frame").start_address().as_u64();
            let tx_desc_virt = phys_to_direct(tx_desc_frame);
            dev.tx_desc_virt = tx_desc_virt;
            let _tx_descs =
                core::slice::from_raw_parts_mut(tx_desc_virt as *mut TxDesc, NUM_TX_DESC);

            for _ in 0..NUM_TX_DESC {
                let buf_frame = alloc_frame().expect("e1000 tx buf frame").start_address().as_u64();
                dev.tx_buffers.push(buf_frame);
            }

            dev.write_reg(REG_TDBAL, tx_desc_frame as u32);
            dev.write_reg(REG_TDBAH, (tx_desc_frame >> 32) as u32);
            dev.write_reg(
                REG_TDLEN,
                (NUM_TX_DESC * core::mem::size_of::<TxDesc>()) as u32,
            );
            dev.write_reg(REG_TDH, 0);
            dev.write_reg(REG_TDT, 0);
            dev.write_reg(REG_TCTL, (1 << 1) | (1 << 3) | (0x0F << 4) | (0x40 << 12));

            let ctrl = dev.read_reg(REG_CTRL);
            dev.write_reg(REG_CTRL, ctrl | (1 << 6));
        }

        dev
    }

    pub fn receive(&self) -> Option<Vec<u8>> {
        let cur = self.rx_cur.load(Ordering::Relaxed);
        let desc = unsafe { &mut *(self.rx_desc_virt as *mut RxDesc).add(cur) };

        if desc.status & 1 == 0 {
            return None;
        }

        let len = desc.length as usize;
        let buf_virt = phys_to_direct(self.rx_buffers[cur]);
        let data = unsafe { core::slice::from_raw_parts(buf_virt as *const u8, len).to_vec() };

        desc.status = 0;
        let next = (cur + 1) % NUM_RX_DESC;
        self.rx_cur.store(next, Ordering::Relaxed);
        unsafe {
            self.write_reg(REG_RDT, cur as u32);
        }

        Some(data)
    }

    pub fn transmit(&self, packet: &[u8]) -> bool {
        if packet.len() > BUF_SIZE {
            return false;
        }

        let cur = self.tx_cur.load(Ordering::Relaxed);
        let desc = unsafe { &mut *(self.tx_desc_virt as *mut TxDesc).add(cur) };

        if desc.cmd != 0 && desc.status & 1 == 0 {
            return false;
        }

        let buf_virt = phys_to_direct(self.tx_buffers[cur]);
        unsafe {
            core::ptr::copy_nonoverlapping(packet.as_ptr(), buf_virt as *mut u8, packet.len());
        }

        desc.addr = self.tx_buffers[cur];
        desc.length = packet.len() as u16;
        desc.cmd = (1 << 0) | (1 << 1) | (1 << 3);
        desc.status = 0;

        let next = (cur + 1) % NUM_TX_DESC;
        self.tx_cur.store(next, Ordering::Relaxed);
        unsafe {
            self.write_reg(REG_TDT, next as u32);
        }

        true
    }
}
