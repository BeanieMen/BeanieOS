use spin::Once;

pub struct Madt {
    pub lapic_address: usize,
    pub ioapic_address: usize,
    pub ioapic_gsi_base: u32,
    pub keyboard_gsi: u32,
}

static MADT: Once<Madt> = Once::new();

unsafe fn read_u8(addr: usize) -> u8 {
    unsafe { core::ptr::read_unaligned(addr as *const u8) }
}

unsafe fn read_u32(addr: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(addr as *const u32) }
}

pub(crate) unsafe fn init(root: usize) {
    let madt = unsafe { find_madt(root) };
    let parsed = unsafe { parse_madt(madt) };

    MADT.call_once(|| parsed);
}

pub(crate) fn get() -> &'static Madt {
    MADT.get().expect("MADT not initialized")
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

unsafe fn parse_madt(madt: usize) -> Madt {
    let length = unsafe { read_u32(madt + 4) } as usize;

    // MADT:
    // +36: Local APIC address
    // +40: flags
    let lapic_address = unsafe { read_u32(madt + 36) } as usize;

    let mut ioapic_address = None;
    let mut ioapic_gsi_base = None;
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

                ioapic_address = Some(address);
                ioapic_gsi_base = Some(gsi_base);
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

    Madt {
        lapic_address,
        ioapic_address: ioapic_address.expect("No I/O APIC"),
        ioapic_gsi_base: ioapic_gsi_base.expect("No I/O APIC GSI base"),
        keyboard_gsi,
    }
}
