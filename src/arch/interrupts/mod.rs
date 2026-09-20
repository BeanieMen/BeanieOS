pub mod consts;
mod faults;
pub mod pic;
mod vectors;

use lazy_static::lazy_static;
use x86_64::structures::idt::InterruptDescriptorTable;

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        faults::register_faults(&mut idt);
        vectors::register_vectors(&mut idt);
        idt
    };
}

pub fn init_idt(acpi_root_addr: usize) {
    IDT.load();
    unsafe {
        pic::init(acpi_root_addr);
    }
}
