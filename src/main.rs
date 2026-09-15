#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;
mod interrupts;
mod mem;
mod vga_buffer;
mod gdt;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!");
    init();

    fn stack_overflow() {
        stack_overflow();
    }
    stack_overflow();

    println!("It did not crash!");
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

pub fn init() {
    interrupts::init_idt();
    gdt::init();
}
