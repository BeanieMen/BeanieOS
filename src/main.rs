#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use fatfs::Read as _;

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

    // Bring up the disk controller, then read the filesystem through it.
    match boot_disk() {
        Ok(()) => println!("done"),
        Err(why) => println!("disk: {why}"),
    }

    loop {
        x86_64::instructions::hlt();
    }
}

/// Enumerate the controller, mount the filesystem and print a file from it.
fn boot_disk() -> Result<(), &'static str> {
    let devices = arch::pci::find_ahci();
    if devices.is_empty() {
        return Err("no AHCI controller found");
    }

    let device = &devices[0];
    let (bar5, size) = device.bar5_info().ok_or("no usable BAR5")?;
    let bus = device.address.bus();
    let slot = device.address.device();
    let function = device.address.function();
    println!("controller {bus:02x}:{slot:02x}.{function} BAR5={bar5:#x} size {size:#x}");

    let mut controller = arch::ahci::AhciController::new(device)?;
    let total = controller.size();
    println!("disk ready: {total} bytes, {} byte sectors", controller.sector_size());

    // Sector 0 is the protective master boot record, so read sector 1 to prove
    // the path works before involving FAT.
    let mut first = [0u8; 512];
    controller.read_at(512, &mut first)?;
    println!("sector 1 starts {:02x?}", &first[..4]);

    let filesystem = fs::mount(controller)?;
    let root = filesystem.root_dir();

    println!("root directory:");
    let mut files = 0;
    for entry in root.iter() {
        let entry = entry.map_err(|_| "directory read failed")?;
        let name = entry.file_name();
        let kind = if entry.is_dir() { "dir " } else { "file" };
        println!("  {kind} {name} ({} bytes)", entry.len());

        if !entry.is_file() {
            continue;
        }
        files += 1;
        let mut file = root.open_file(&name).map_err(|_| "could not open file")?;
        let mut text = [0u8; 256];
        let read = file.read(&mut text).map_err(|_| "file read failed")?;
        match core::str::from_utf8(&text[..read.min(text.len())]) {
            Ok(body) => println!("      {body}"),
            Err(_) => println!("      <{read} bytes of binary data>"),
        }
    }

    println!("{files} file(s)");

    Ok(())
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
            mbi_addr as u64 + mbi_size as usize as u64,
        )
    };

    let mut mapper = unsafe { memory::allocator::init(VirtAddr::new(0)) };

    memory::heap::init_heap(&mut mapper, &mut frame_alloc).expect("heap initialization failed");

    graphics::framebuffer::init_framebuffer(
        fb_tag.address(),
        fb_tag.width(),
        fb_tag.height(),
        fb_tag.pitch(),
        fb_tag.bpp(),
    );

    arch::gdt::init();
    arch::interrupts::init_idt(acpi_root_addr_from(boot_info));
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

pub async fn testlol() {
    println!("testlol");
}
