#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use alloc::boxed::Box;
use core::panic::PanicInfo;

use fatfs::FileSystem;

use multiboot2::BootInformation;
use x86_64::VirtAddr;

extern crate alloc;

mod arch;
mod console;
mod fs;
mod graphics;
mod hal;
mod mem;
mod memory;
mod shell;
mod syscall;
mod task;
mod userspace;

fn kernel_main(boot_info: BootInformation<'_>, mbi_addr: u32, mbi_size: usize) -> ! {
    init(&boot_info, mbi_addr, mbi_size);

    println!("BeanieOS");
    println!("heap ready");
    println!("framebuffer ready");

    let disk = boot_disk().unwrap();
    userspace::install(disk);

    print!("> ");

    let init = task::process::create("init", "/");

    task::scheduler::spawn_in_process(
        init,
        "shell",
        userspace::shell_task,
        task::identity::Priority::Normal,
    );

    task::scheduler::spawn_in_process(
        init,
        "rgb_square",
        userspace::rgb_square_task,
        task::identity::Priority::Normal,
    );

    loop {
        task::scheduler::yield_now();
        x86_64::instructions::hlt();
    }
}

fn boot_disk() -> Result<FileSystem<Box<dyn fs::BlockDevice>>, &'static str> {
    let devices = hal::hal().pci.find_ahci();
    if devices.is_empty() {
        return Err("no AHCI controller found");
    }

    let device = &devices[0];
    let (bar5, size) = device.bar5_info().ok_or("no usable BAR5")?;
    let bus = device.address.bus();
    let slot = device.address.device();
    let function = device.address.function();
    println!("controller {bus:02x}:{slot:02x}.{function} BAR5={bar5:#x} size {size:#x}");

    println!(
        "ticks before AHCI = {}",
        crate::arch::interrupts::vectors::ticks()
    );

    let mut controller = arch::ahci::AhciController::new(device)?;

    println!(
        "ticks after AHCI = {}",
        crate::arch::interrupts::vectors::ticks()
    );
    let total = controller.size();
    println!(
        "disk ready: {total} bytes, {} byte sectors",
        controller.sector_size()
    );

    // Sector 0 is the MBR, so sector 1 proves the path before involving FAT.
    let mut first = [0u8; 512];
    controller.read_at(512, &mut first)?;
    println!("sector 1 starts {:02x?}", &first[..4]);

    let filesystem = fs::mount(controller)?;
    Ok(filesystem)
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
        memory::allocator::BumpAllocator::init(
            memory_map,
            mbi_addr as u64,
            mbi_addr as u64 + mbi_size as u64,
        )
    };

    let routes = hal::discover();
    memory::allocator::RESERVED.snapshot();
    println!(
        "  {} device range(s) reserved: {}",
        memory::allocator::RESERVED.len(),
        routes.count()
    );

    let mmio_mapper = unsafe { memory::allocator::mapper(VirtAddr::new(0)) };
    let mut heap_mapper = unsafe { memory::allocator::mapper(VirtAddr::new(0)) };

    memory::heap::init_heap(&mut heap_mapper, &mut frame_alloc)
        .expect("heap initialization failed");

    hal::init(mmio_mapper, &routes, &mut frame_alloc);

    graphics::framebuffer::init_framebuffer(
        fb_tag.address(),
        fb_tag.width(),
        fb_tag.height(),
        fb_tag.pitch(),
        fb_tag.bpp(),
    );

    arch::gdt::init();
    arch::interrupts::init_idt(acpi_root_addr_from(boot_info));
    x86_64::instructions::interrupts::enable();

    task::process::init();
    task::scheduler::init();
}

fn acpi_root_addr_from(boot_info: &BootInformation<'_>) -> usize {
    if let Some(rsdp) = boot_info.rsdp_v2_tag() {
        rsdp.xsdt_address()
    } else if let Some(rsdp) = boot_info.rsdp_v1_tag() {
        rsdp.rsdt_address()
    } else {
        panic!("No ACPI RSDP")
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{info}");
    loop {
        x86_64::instructions::hlt();
    }
}
