use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::VirtAddr;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::InterruptDescriptorTable;

use super::consts::{
    AHCI_VECTOR, KEYBOARD_VECTOR, LAPIC_TIMER_VECTOR, LEGACY_PIC0_COMMAND, MOUSE_VECTOR, PIC_EOI,
    PIC_MASTER_SPURIOUS_VECTOR, PIC_MOUSE_VECTOR, PIC_SLAVE_SPURIOUS_VECTOR, SPURIOUS_VECTOR,
};
use super::pic;

static TICKS: AtomicU64 = AtomicU64::new(0);
static KEYS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn timer_interrupt_entry();
}

pub(crate) fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub(crate) fn keys() -> u64 {
    KEYS.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
extern "C" fn timer_interrupt_rust(cs: u64) {
    let tick = TICKS.fetch_add(1, Ordering::Relaxed);
    if tick.is_multiple_of(8) {
        unsafe {
            let mut lsr = Port::<u8>::new(0x3fd);
            while lsr.read() & 1 != 0 {
                let mut data = Port::<u8>::new(0x3f8);
                let byte = data.read();
                let ch = match byte {
                    b'\r' => b'\n',
                    0x7f | 0x08 => 0x08,
                    b => b,
                };
                crate::tty::push_byte(ch);
                KEYS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    crate::task::scheduler::account_tick();
    unsafe {
        pic::eoi();
    }

    if (cs & 3) == 3 && crate::task::scheduler::is_reschedule_requested() {
        crate::task::scheduler::yield_now();
    }
}

extern "x86-interrupt" fn generic_interrupt_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    unsafe {
        pic::eoi();
    }
}

extern "x86-interrupt" fn spurious_interrupt_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
}

extern "x86-interrupt" fn pic_master_spurious_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    unsafe {
        pic::lapic::send_eoi();
    }
}

extern "x86-interrupt" fn pic_slave_spurious_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    unsafe {
        Port::<u8>::new(LEGACY_PIC0_COMMAND).write(PIC_EOI);
        pic::lapic::send_eoi();
    }
}

extern "x86-interrupt" fn ps2_interrupt_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    crate::arch::ps2::on_interrupt();
    unsafe {
        pic::eoi();
    }
}

pub(crate) fn register_vectors(idt: &mut InterruptDescriptorTable) {
    for vector in 32..=255u8 {
        idt[vector].set_handler_fn(generic_interrupt_handler);
    }

    idt[KEYBOARD_VECTOR].set_handler_fn(ps2_interrupt_handler);
    idt[MOUSE_VECTOR].set_handler_fn(ps2_interrupt_handler);
    idt[PIC_MOUSE_VECTOR].set_handler_fn(ps2_interrupt_handler);
    idt[PIC_MASTER_SPURIOUS_VECTOR].set_handler_fn(pic_master_spurious_handler);
    idt[PIC_SLAVE_SPURIOUS_VECTOR].set_handler_fn(pic_slave_spurious_handler);
    idt[SPURIOUS_VECTOR].set_handler_fn(spurious_interrupt_handler);
    idt[AHCI_VECTOR].set_handler_fn(crate::storage::ahci::on_interrupt);

    unsafe {
        idt[LAPIC_TIMER_VECTOR]
            .set_handler_addr(VirtAddr::from_ptr(timer_interrupt_entry as *const ()));
    }
}
