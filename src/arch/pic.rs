const KEYBOARD_VECTOR: u8 = 33;
const LAPIC_DEFAULT: usize = 0xFEE0_0000;

struct IoApic {
    address: usize,
    gsi_base: u32,
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

unsafe fn find_madt(root: usize) -> usize {
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

unsafe fn parse_madt(madt: usize) -> (IoApic, u32) {
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

                if bus == 0 && source == 1 {
                    keyboard_gsi = gsi;
                }
            }

            _ => {}
        }

        offset += len as usize;
    }

    (ioapic.expect("No I/O APIC"), keyboard_gsi)
}

unsafe fn init_lapic() {
    let svr = (LAPIC_DEFAULT + 0xF0) as *mut u32;

    let value = unsafe { core::ptr::read_volatile(svr) };

    unsafe {
        core::ptr::write_volatile(svr, value | 0x100 | 0xFF);
    }
}

unsafe fn init_ioapic(ioapic: IoApic, keyboard_gsi: u32) {
    let index = keyboard_gsi - ioapic.gsi_base;

    let low = 0x10 + index * 2;
    let high = low + 1;

    let lapic_id = unsafe { core::ptr::read_volatile((LAPIC_DEFAULT + 0x20) as *const u32) } >> 24;

    unsafe {
        ioapic_write(ioapic.address, high as u8, lapic_id << 24);
    }

    unsafe {
        ioapic_write(ioapic.address, low as u8, KEYBOARD_VECTOR as u32);
    }
}

fn disable_legacy_pic() {
    // 0x21/0xA1 are I/O *ports*, not MMIO addresses: they must be written
    // with `out`, not with a plain memory store. A store to address 0x21
    // would just corrupt low RAM (real-mode IVT) and leave the PIC live,
    // so its timer/keyboard IRQs would keep firing on vectors that have no
    // IDT entry -> #NP/#GP -> double fault.
    use x86_64::instructions::port::Port;
    unsafe {
        Port::new(0x21).write(0xFFu8);
        Port::new(0xA1).write(0xFFu8);
    }
}

pub unsafe fn init(acpi_root_addr: usize) {
    let madt = unsafe { find_madt(acpi_root_addr) };

    let (ioapic, keyboard_gsi) = unsafe { parse_madt(madt) };

    unsafe {
        init_lapic();
    }
    unsafe {
        init_ioapic(ioapic, keyboard_gsi);
    }

    disable_legacy_pic();
}

pub unsafe fn eoi() {
    unsafe {
        core::ptr::write_volatile((LAPIC_DEFAULT + 0xB0) as *mut u32, 0);
    }
}
