#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use bootloader::{BootInfo, entry_point};
use core::panic::PanicInfo;
use x86_64::{
    VirtAddr,
    structures::paging::{Page, PageTable, Translate},
};
mod allocator;
mod gdt;
mod interrupts;
mod mem;
mod memory;
mod vga_buffer;

extern crate alloc;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    println!("Hello World{}", "!");
    init();

    let physical_memory_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(physical_memory_offset) };
    let mut frame_allocator = memory::EmptyFrameAllocator;
    allocator::init_heap(&mut mapper, &mut frame_allocator);
    // map an unused page
    let page: Page = Page::containing_address(VirtAddr::new(0x0));
    memory::create_example_mapping(page, &mut mapper, &mut frame_allocator);

    // write the string `New!` to the screen through the new mapping
    let page_ptr: *mut u64 = page.start_address().as_mut_ptr();
    println!("page_ptr: {:?}", page_ptr);
    unsafe { page_ptr.offset(400).write_volatile(0x_f021_f077_f065_f04e) };
    let x = alloc::boxed::Box::new(0);
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
