#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use multiboot2::BootInformation;

extern crate alloc;

mod arch;
mod console;
mod graphics;
mod hal;
mod logger;
mod mem;
mod memory;
mod storage;
mod syscall;
mod task;
mod userspace;

fn kernel_main(boot_info: BootInformation<'_>, mbi_addr: u32, mbi_size: usize) -> ! {
    init(&boot_info, mbi_addr, mbi_size);

    for mut disk in storage::ahci::find_disks() {
        kinfo!(
            "disk {}: {} {} ({} sectors of {} bytes)",
            disk.port(),
            disk.model(),
            disk.serial(),
            disk.sectors(),
            disk.sector_size()
        );

        for (index, partition) in disk.partitions().iter().enumerate() {
            kinfo!(
                "  partition {index}: lba {} ({} sectors, {} bytes at {:#x})",
                partition.first_lba(),
                partition.sectors(),
                partition.byte_len(),
                partition.byte_offset()
            );
        }
    }

    let init = task::process::create("init", "/");

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

pub fn init(boot_info: &BootInformation<'_>, mbi_addr: u32, mbi_size: usize) {
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

    let routes = hal::discover();
    memory::allocator::RESERVED.snapshot();

    let mut mmu = unsafe {
        memory::mmu::MMU::boot(
            memory_map,
            mbi_addr as u64,
            mbi_addr as u64 + mbi_size as u64,
        )
    };

    mmu.init_heap();
    hal::init(&mut mmu, &routes);

    for entry in routes.iter() {
        let Some((bar5, size)) = entry.bar5_info() else {
            continue;
        };
        kinfo!("found device: {:#x}..{:#x}", bar5, bar5 + size);
    }

    arch::gdt::init();
    arch::interrupts::init_idt(boot_info);
    x86_64::instructions::interrupts::enable();

    // Log only after init: logging earlier splits init across log lines.
    kinfo!(
        "Boot memory map found with {} regions",
        memory_map.memory_areas().len()
    );

    kinfo!("framebuffer found at {}", fb_tag.address());

    kinfo!(
        "  {} device range(s) reserved: {}",
        memory::allocator::RESERVED.len(),
        routes.count()
    );

    kinfo!(
        "heap initialized at {:#x}..{:#x}",
        mmu.heap_range().0,
        mmu.heap_range().1
    );
    kinfo!("kernel initialized, starting init process");

    task::process::init();
    task::scheduler::init();
}

#[panic_handler]

fn panic(info: &PanicInfo) -> ! {
    kerror!("{info}");
    loop {
        x86_64::instructions::hlt();
    }
}
