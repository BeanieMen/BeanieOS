pub mod consts;
mod faults;

use spin::Once;
use x86_64::structures::idt::InterruptDescriptorTable;

static IDT: Once<InterruptDescriptorTable> = Once::new();

fn idt() -> &'static InterruptDescriptorTable {
    IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        faults::register_faults(&mut idt);
        idt
    })
}

pub(crate) fn idt_addr() -> u64 {
    &raw const *idt() as u64
}

pub(crate) fn init_idt() {
    idt().load();
}