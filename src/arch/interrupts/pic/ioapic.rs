use super::super::consts::KEYBOARD_VECTOR;
use super::lapic;

pub(super) struct IoApic {
    pub(super) address: usize,
    pub(super) gsi_base: u32,
}

unsafe fn read_u8(addr: usize) -> u8 {
    unsafe { core::ptr::read_unaligned(addr as *const u8) }
}

unsafe fn read_u32(addr: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(addr as *const u32) }
}

unsafe fn ioapic_write(base: usize, reg: u8, value: u32) {
    unsafe {
        core::ptr::write_volatile(base as *mut u32, reg as u32);
        core::ptr::write_volatile((base + 0x10) as *mut u32, value);
    }
}

pub(super) unsafe fn find_madt(root: usize) -> usize {
    let signature = unsafe { core::slice::from_raw_parts(root as *const u8, 4) };
    let entry_size = if signature == b"XSDT" { 8 } else { 4 };

    let length = unsafe { read_u32(root + 4) } as usize;
    let mut offset = root + 36;
    let end = root + length;

    while offset < end {
        let table = if entry_size == 8 {
            unsafe { core::ptr::read_unaligned(offset as *const u64) as usize }
        } else {
            unsafe { read_u32(offset) as usize }
        };

        let signature = unsafe { core::slice::from_raw_parts(table as *const u8, 4) };
        if signature == b"APIC" {
            return table;
        }

        offset += entry_size;
    }

    panic!("No MADT");
}

pub(super) unsafe fn parse_madt(madt: usize) -> (IoApic, u32) {
    let length = unsafe { read_u32(madt + 4) } as usize;

    let mut ioapic = None;
    let mut keyboard_gsi = 1u32;

    let mut offset = madt + 44;
    let end = madt + length;

    while offset < end {
        let typ = unsafe { read_u8(offset) };
        let len = unsafe { read_u8(offset + 1) };

        if len < 2 {
            break;
        }

        match typ {
            // I/O APIC
            1 => {
                let address = unsafe { read_u32(offset + 4) } as usize;
                let gsi_base = unsafe { read_u32(offset + 8) };
                ioapic = Some(IoApic { address, gsi_base });
            }
            // Interrupt Source Override
            2 => {
                let bus = unsafe { read_u8(offset + 2) };
                let source = unsafe { read_u8(offset + 3) };
                let gsi = unsafe { read_u32(offset + 4) };
                if bus == 0 && source == 1 { // legacy keyboard IRQ
                    keyboard_gsi = gsi;
                }
            }
            _ => {}
        }

        offset += len as usize;
    }

    (ioapic.expect("No I/O APIC"), keyboard_gsi)
}

pub(super) unsafe fn init(ioapic: IoApic, keyboard_gsi: u32) {
    let index = keyboard_gsi - ioapic.gsi_base;
    // calculates the low and high register offsets to setup I/O APIC redirection table entry corresponding to the keyboard GSI using its index
    // which is alo then calculated using keyboard gsi parsed from madt and gsi base of the I/O APIC.
    // demonic level of confusion caused by legacy stuff once again
    let low = 0x10 + index * 2;
    let high = low + 1;

    let lapic_id = unsafe { lapic::id() };

    unsafe {
        ioapic_write(ioapic.address, high as u8, lapic_id << 24);
    }
    unsafe {
        ioapic_write(ioapic.address, low as u8, KEYBOARD_VECTOR as u32);
    }
}
