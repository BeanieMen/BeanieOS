#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use multiboot2::BootInformation;
use x86_64::VirtAddr;

extern crate alloc;

mod arch;
mod fs;
mod graphics;
mod mem;
mod memory;
mod task;

fn kernel_main(boot_info: BootInformation<'_>, mbi_addr: u32, mbi_size: usize) -> ! {
    init(&boot_info, mbi_addr, mbi_size);

    println!("BeanieOS");
    println!("heap ready");
    println!("framebuffer ready");

    graphics::framebuffer::WRITER.lock().fill_rect(
        100,
        100,
        50,
        50,
        crate::graphics::framebuffer::Color::Red.to_rgb(),
    );

    for device in arch::pci::find_ahci() {
        let Some((bar5, size)) = device.bar5_info() else {
            println!("AHCI has no usable BAR5");
            continue;
        };

        println!(
            "AHCI controller {:02x}:{:02x}.{}",
            device.address.bus(),
            device.address.device(),
            device.address.function()
        );

        println!("BAR5: {:#x}, size {:#x}", bar5, size);

        if bar5 >= 8 * 1024 * 1024 * 1024 {
            println!("BAR5 is outside identity-mapped range");
            continue;
        }

        device.enable();

        println!("PCI MMIO + bus mastering enabled");

        if let Some(mut ahci) = arch::ahci::init(&device) {
            println!("AHCI initialized");
            println!("Block size: {}", ahci.block_size());
            println!("Capacity: {} blocks", ahci.capacity());

            if ahci.capacity() == 0 {
                println!("No disk on this controller");
                continue;
            }

            println!("Disk controller found");

            let mut sector = [0u8; 512];

            if ahci.read(0, &mut sector) {
                println!("Sector 0:");

                for byte in &sector[..16] {
                    print!("{:02x} ", byte);
                }

                println!();
            } else {
                println!("Sector 0 read failed");
            }

            break;
        }
    }
    loop {
        x86_64::instructions::hlt();
    }
    // let mut executor = executor::Executor::new();
    // executor aint needed for now
    // executor.spawn(executor::Task::new(testlol()));
    // executor.run();
}

pub fn init(boot_info: &BootInformation<'_>, mbi_addr: u32, mbi_size: usize) {
    let memory_map = boot_info
        .memory_map_tag()
        .expect("No Multiboot2 memory map");

    let fb_tag = match boot_info.framebuffer_tag() {
        Some(Ok(tag)) => tag,
        Some(Err(_)) => panic!("Invalid Multiboot2 framebuffer"),
        None => panic!("No Multiboot2 framebuffer"),
    };

    let mut frame_alloc = unsafe {
        memory::allocator::Multiboot2FrameAllocator::init(
            memory_map,
            mbi_addr as u64,
            mbi_addr as u64 + mbi_size as u64,
        )
    };

    let mut mapper = unsafe { memory::allocator::init(VirtAddr::new(0)) };

    let acpi_root_addr = if let Some(rsdp) = boot_info.rsdp_v2_tag() {
        rsdp.xsdt_address()
    } else if let Some(rsdp) = boot_info.rsdp_v1_tag() {
        rsdp.rsdt_address()
    } else {
        panic!("No ACPI RSDP");
    };

    memory::heap::init_heap(&mut mapper, &mut frame_alloc).expect("heap initialization failed");

    graphics::framebuffer::init_framebuffer(
        fb_tag.address(),
        fb_tag.width(),
        fb_tag.height(),
        fb_tag.pitch(),
        fb_tag.bpp(),
    );

    arch::gdt::init();
    arch::interrupts::init_idt(acpi_root_addr);

    x86_64::instructions::interrupts::enable();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);

    loop {
        x86_64::instructions::hlt();
    }
}

pub async fn testlol() {
    println!("testlol");
}
