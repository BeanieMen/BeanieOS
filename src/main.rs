#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use bootloader::{BootInfo, entry_point};
use x86_64::VirtAddr;
use core::panic::PanicInfo;
mod gdt;
mod interrupts;
mod memory;
mod vga_buffer;
mod mem;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    println!("Hello World{}", "!");
    init();

    use x86_64::registers::control::Cr3;

    let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let level_4_table = unsafe { memory::active_level_4_table(physical_memory_offset) };
    println!("Level 4 page table at: {:p}", level_4_table);

    for (i, entry) in level_4_table.iter().enumerate() {
        if !entry.is_unused() {
            println!("l4 entry {}: {:?}", i, entry);
        }
    }

    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

pub fn init() {
    gdt::init();
    interrupts::init_idt();
    unsafe {
        interrupts::PICS.lock().initialize();
    }
    x86_64::instructions::interrupts::enable();
}
