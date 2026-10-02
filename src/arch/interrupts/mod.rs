pub mod consts;
mod faults;
pub mod pic;
pub mod vectors;

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

pub fn init_idt(acpi_root_addr: usize) {
    idt().load();
    unsafe {
        pic::init(acpi_root_addr);
    }
}
