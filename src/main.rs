#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use multiboot2::BootInformation;

use crate::storage::{ahci::disk::partition, vfs::mount::FileSystem};

extern crate alloc;

mod arch;
mod console;
mod graphics;
mod hal;
mod logger;
mod memory;
mod paging;
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
        let partitions = disk.partitions();
        for (index, partition) in partitions.iter().enumerate() {
            kinfo!(
                "  partition {index}: lba {} ({} sectors, {} bytes at {:#x})",
                partition.first_lba(),
                partition.sectors(),
                partition.byte_len(),
                partition.byte_offset()
            );
        }

        let partition_lba = partitions[1].first_lba();
        let partition_len = partitions[1].sectors();
        kinfo!("mounting partition at LBA {}", partition_lba);

        let fat32 = storage::fs::fat32::Fat32::mount(partition::Partition::new(
            disk,
            partition_lba,
            partition_len,
        ))
        .expect("failed to mount FAT32");

        kinfo!("FAT32 mounted at LBA {}", partition_lba);

        let fd = task::process::with_current_fds(|fds| {
            fds.mount(fat32.clone());
            fds.open_read(b"/TEST.TXT")
        })
        .expect("no current process")
        .expect("failed to open /TEST.TXT");

        kinfo!("opened /TEST.TXT as fd {fd}");

        let mut buf = [0u8; 256];

        let n = task::process::with_current_fds(|fds| fds.read(fd, &mut buf))
            .expect("no current process")
            .expect("failed to read /TEST.TXT");

        kinfo!("TEST.TXT: {} bytes", n);

        for &byte in &buf[..n] {
            print!("{}", byte as char);
        }

        let cursor = task::process::with_current_fds(|fds| fds.get(fd).map(|f| f.tell()))
            .expect("no current process")
            .expect("fd vanished");

        let closed = task::process::with_current_fds(|fds| fds.close(fd));

        kinfo!("fd {fd} cursor at {cursor}, close gave {closed:?}");

        break;
    }
    let init = task::process::create("init", "/");

    task::scheduler::spawn_in_process(
        init,
        "rgb_square",
        userspace::rgb_square_task,
        task::identity::Priority::Normal,
    );

    let user = task::process::create("user", "/");

    task::scheduler::spawn_in_process(
        user,
        "usertask",
        user_task,
        task::identity::Priority::Normal,
    );

    loop {
        task::scheduler::yield_now();
        x86_64::instructions::hlt();
    }
}

extern "C" fn user_task() {
    match task::userland::verify_release() {
        Ok(()) => kinfo!("address space release: no frames leaked"),
        Err(why) => kerror!("address space release: {why}"),
    }

    let Some(space) = task::userland::build() else {
        kerror!("could not build a user address space");
        loop {
            task::scheduler::yield_now();
        }
    };

    task::process::attach_space(task::identity::current_pid(), space.root);

    kinfo!("entering ring 3 at {:#x}", task::userland::USER_BASE);

    task::userland::enter(&space)
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

    let mut mmu = unsafe {
        memory::mmu::MMU::boot(
            memory_map,
            mbi_addr as u64,
            mbi_addr as u64 + mbi_size as u64,
        )
    };

    mmu.init_heap().expect("could not map the kernel heap");
    hal::init(&mut mmu);

    let routes = hal::routes();

    for entry in routes.iter() {
        let Some((bar5, size)) = entry.bar5_info() else {
            continue;
        };
        kinfo!("found device: {:#x}..{:#x}", bar5, bar5 + size);
    }

    arch::gdt::init();
    arch::interrupts::init_idt(boot_info);
    syscall::entry::install();
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
