pub mod consts;
mod faults;
pub mod pic;
pub mod vectors;

use multiboot2::BootInformation;
use spin::Once;
use x86_64::structures::idt::InterruptDescriptorTable;

static IDT: Once<InterruptDescriptorTable> = Once::new();

fn idt() -> &'static InterruptDescriptorTable {
    IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        faults::register_faults(&mut idt);
        vectors::register_vectors(&mut idt);
        idt
    })
}

pub(crate) fn idt_addr() -> u64 {
    &raw const *idt() as u64
}

fn acpi_root_addr(boot_info: &BootInformation<'_>) -> usize {
    if let Some(rsdp) = boot_info.rsdp_v2_tag() {
        rsdp.xsdt_address()
    } else if let Some(rsdp) = boot_info.rsdp_v1_tag() {
        rsdp.rsdt_address()
    } else {
        panic!("No ACPI RSDP")
    }
}

pub(crate) fn init_idt(boot_info: &BootInformation<'_>) {
    idt().load();

    crate::kinfo!("IDT at {:#x}", idt_addr());

    unsafe {
        pic::init(acpi_root_addr(boot_info));
    }

    crate::arch::ps2::init();
}

pub(crate) fn timer_ticks() -> u64 {
    vectors::ticks()
}

pub(crate) fn key_count() -> u64 {
    vectors::keys()
}
