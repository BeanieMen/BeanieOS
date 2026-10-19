#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![allow(dead_code)]

extern crate alloc;

use core::panic::PanicInfo;

use multiboot2::BootInformation;

mod arch;
mod audio;
mod console;
mod graphics;
mod hal;
mod logger;
mod memory;
mod paging;
mod storage;
mod syscall;
mod task;
mod tty;

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
        match multiboot2::BootInformation::load(
            mbi_addr as *const multiboot2::BootInformationHeader,
        ) {
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

    x86_64::instructions::interrupts::enable();

    task::process::init();
    task::scheduler::init();

    mount_disk();

    let init = task::process::create("init", "/");

    task::scheduler::spawn_in_process(
        init,
        "storage",
        storage_task,
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

type Fat = alloc::sync::Arc<storage::fs::fat32::Fat32<storage::ahci::disk::partition::Partition>>;

static MOUNTED: spin::Mutex<Option<Fat>> = spin::Mutex::new(None);

fn mount_disk() {
    let mut disks = storage::ahci::find_disks();

    let Some(disk) = disks.first_mut() else {
        kerror!("storage: no disk attached");
        return;
    };

    let partitions = disk.partitions();

    for (index, partition) in partitions.iter().enumerate() {
        kinfo!(
            "  partition {index}: lba {} ({} sectors)",
            partition.first_lba(),
            partition.sectors()
        );
    }

    let Some(partition) = partitions.get(1) else {
        kerror!("storage: disk has no second partition");
        return;
    };

    let (lba, sectors) = (partition.first_lba(), partition.sectors());

    match storage::fs::fat32::Fat32::mount(storage::ahci::disk::partition::Partition::new(
        *disk, lba, sectors,
    )) {
        Ok(fat32) => {
            kinfo!("FAT32 mounted at LBA {lba}");

            *MOUNTED.lock() = Some(fat32);
        }
        Err(why) => kerror!("FAT32 mount failed: {why}"),
    }
}

extern "C" fn storage_task() {
    let Some(fat32) = MOUNTED.lock().clone() else {
        kerror!("storage: nothing mounted");
        loop {
            task::scheduler::yield_now();
        }
    };

    let opened = task::process::with_current_fds(|fds| {
        fds.mount(fat32.clone());
        fds.open_read(b"/TEST.TXT")
    });

    match opened {
        Some(Ok(fd)) => {
            kinfo!("opened /TEST.TXT as fd {fd}");

            let mut buf = [0u8; 256];

            let read = task::process::with_current_fds(|fds| fds.read(fd, &mut buf));

            match read {
                Some(Ok(n)) => {
                    kinfo!("/TEST.TXT: {n} bytes");

                    for &byte in &buf[..n] {
                        print!("{}", byte as char);
                    }

                    print!("\n");
                }
                Some(Err(why)) => kerror!("/TEST.TXT read: {why}"),
                None => kerror!("no current process"),
            }
        }
        Some(Err(why)) => kerror!("/TEST.TXT: {why}"),
        None => kerror!("no current process to mount into"),
    }

    loop {
        task::scheduler::yield_now();

        let got = task::process::with_current_fds(|fds| {
            let fd = fds.open_read(b"/OUT.TXT").ok()?;

            let mut buf = [0u8; 2048];

            fds.read(fd, &mut buf).ok().map(|n| (n, buf))
        });

        let Some(Some((n, buf))) = got else {
            task::scheduler::sleep(10);
            continue;
        };

        if n >= 5 && &buf[..5] == b"RING3" {
            kinfo!("ring 3 wrote this to the disk through syscalls:");

            for &byte in &buf[..n] {
                print!("{}", byte as char);
            }

            print!("\n");

            break;
        }

        if n >= 1200 {
            let expected = (0..1200)
                .map(|i| b'A' + (i % 26) as u8)
                .collect::<alloc::vec::Vec<u8>>();

            if buf[..1200] == expected[..] {
                kinfo!("ring 3 wrote 1200 bytes across the FAT chain; all correct");
            }

            break;
        }
    }

    loop {
        task::scheduler::yield_now();
    }
}

extern "C" fn user_task() {
    let Some(fat32) = MOUNTED.lock().clone() else {
        kerror!("user: nothing mounted");
        loop {
            task::scheduler::yield_now();
        }
    };

    let mounted = task::process::with_current_fds(|fds| {
        fds.mount(fat32.clone());

        fds.mount_count()
    });

    match mounted {
        Some(count) => kinfo!("user process has {count} mount(s)"),
        None => kerror!("no current process to mount into"),
    }

    match task::userland::verify_release() {
        Ok(()) => kinfo!("address space release: no frames leaked"),
        Err(why) => kerror!("address space release: {why}"),
    }

    let elf_data = task::process::with_current_fds(|fds| {
        let fd = fds.open_read(b"/INIT").ok()?;
        let mut buf = alloc::vec![0u8; 65536];
        let n = fds.read(fd, &mut buf).ok()?;
        buf.truncate(n);
        Some(buf)
    })
    .flatten();

    if let Some(binary) = elf_data {
        let mut space = memory::mmu::AddressSpace::new_with_kernel()
            .expect("could not allocate user address space");

        match task::elf::load_elf(&mut space, &binary) {
            Ok(loaded) => {
                task::process::attach_space(task::identity::current_pid(), space.root);
                kinfo!("loaded /INIT, entering ring 3 at {:#x}", loaded.entry_point);
                space.activate();

                let ss = u64::from(arch::gdt::user_data_selector().0) | 0x3;
                let cs = u64::from(arch::gdt::user_code_selector().0) | 0x3;

                unsafe {
                    task::userland::enter_userland(
                        loaded.entry_point,
                        loaded.user_stack_top,
                        0x202,
                        ss & 0xffff,
                        cs & 0xffff,
                    )
                }
            }
            Err(why) => {
                kerror!("failed to load /INIT: {why}");
            }
        }
    } else {
        kerror!("could not find /INIT on disk, falling back to builtin test bytecode");
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

    let fb_start = fb_tag.address();
    let fb_end = fb_start + fb_tag.pitch() as u64 * fb_tag.height() as u64;

    memory::allocator::reserve_framebuffer(fb_start, fb_end);
    memory::mmu::set_framebuffer_range(fb_start, fb_end);

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

    arch::gdt::init();
    arch::interrupts::init_idt(boot_info);
    syscall::entry::install();
    memory::mmu::init_pat();

    demo_direct_map();

    kinfo!(
        "Boot memory map found with {} regions",
        memory_map.memory_areas().len()
    );

    kinfo!("framebuffer found at {:#x}", fb_tag.address());

    for route in hal::routes().iter() {
        if let Some((bar5, size)) = route.bar5_info() {
            match hal::hal().mmio.mapping_for(bar5) {
                Some(m) => kinfo!(
                    "found device: {:#x}..{:#x} mapped at {:#x}",
                    bar5,
                    bar5 + size,
                    m.va
                ),
                None => kinfo!("found device: {:#x}..{:#x} NOT MAPPED", bar5, bar5 + size),
            }
        }
    }
    audio::init();
    kinfo!(
        "heap initialized at {:#x}..{:#x}",
        mmu.heap_range().0,
        mmu.heap_range().1
    );

    kinfo!("kernel initialized");

    let info = memory::mmu::memory_info();
    kinfo!(
        "  framebuffer {:#x}..{:#x}",
        info.framebuffer.expect("set at boot").0,
        info.framebuffer.expect("set at boot").1
    );
    kinfo!("  kernel     {:#x}..{:#x}", info.kernel.0, info.kernel.1);
    kinfo!(
        "  boot info  {:#x}..{:#x}",
        info.boot_info.0,
        info.boot_info.1
    );
    kinfo!(
        "  heap       {:#x}..{:#x}",
        info.heap.expect("set by init_heap").0,
        info.heap.expect("set by init_heap").1
    );
}

fn demo_direct_map() {
    let Some(frame) = memory::allocator::alloc_frame() else {
        kerror!("no frame to demo with");
        return;
    };

    let phys = frame.start_address().as_u64();
    let virt = paging::phys_to_direct(phys);

    kinfo!("demo frame phys = {phys:#x}");
    kinfo!("demo frame virt = {virt:#x}");
    kinfo!(
        "offset           = {:#x} (DIRECT_MAP_BASE is {:#x})",
        virt.wrapping_sub(phys),
        paging::DIRECT_MAP_BASE
    );

    let ptr = virt as *mut u64;
    unsafe { ptr.write(0xBEEF_CAFE_1234_5678) };

    let readback = unsafe { ptr.read_volatile() };
    kinfo!("wrote/read {readback:#x} through the virtual address");

    if readback != 0xBEEF_CAFE_1234_5678 {
        kerror!("direct map write did not read back");
    } else {
        kinfo!("direct map OK: physical {phys:#x} is usable as {virt:#x}");
    }

    memory::allocator::free_frame(phys);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kerror!("{info}");
    loop {
        x86_64::instructions::hlt();
    }
}
