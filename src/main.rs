#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use multiboot2::BootInformation;

mod arch;
mod console;
mod graphics;
mod logger;
mod paging;

const MULTIBOOT2_MAGIC: u32 = 0x36d76289;

#[unsafe(no_mangle)]
pub extern "C" fn rust_entry(magic: u32, mbi_addr: u32) -> ! {
    if magic != MULTIBOOT2_MAGIC {
        loop {
            x86_64::instructions::hlt();
        }
    }


    let mbi_total_size = unsafe { (mbi_addr as *const u32).read() as usize };

    let boot_info = unsafe {
        match multiboot2::BootInformation::load(mbi_addr as *const multiboot2::BootInformationHeader)
        {
            Ok(info) => info,
            Err(_) => loop {
                x86_64::instructions::hlt();
            },
        }
    };

    kernel_main(boot_info, mbi_addr, mbi_total_size);
}

fn kernel_main(boot_info: BootInformation<'_>, mbi_addr: u32, mbi_size: usize) -> ! {
    init(&boot_info, mbi_addr, mbi_size);
    kinfo!("Kernel initialized successfully");
    loop {
        x86_64::instructions::hlt();
    }
}

pub(crate) fn init(boot_info: &BootInformation<'_>, mbi_addr: u32, mbi_size: usize) {
    let memory_map = boot_info
        .memory_map_tag()
        .expect("No Multiboot2 memory map");

    let fb_tag = match boot_info.framebuffer_tag() {
        Some(Ok(tag)) => tag,
        Some(Err(_)) => panic!("Invalid Multiboot2 framebuffer"),
        None => panic!("No Multiboot2 framebuffer"),
    };

    graphics::framebuffer::init_framebuffer(
        fb_tag.address(),
        fb_tag.width(),
        fb_tag.height(),
        fb_tag.pitch(),
        fb_tag.bpp(),
    );
    arch::gdt::init();
    
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kerror!("{info}");
    loop {
        x86_64::instructions::hlt();
    }
}
