#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use multiboot2::BootInformation;

use x86_64::{
    PhysAddr,
    VirtAddr,
    structures::paging::{Mapper, Page, PageTableFlags, PhysFrame, Size4KiB},
};

extern crate alloc;

mod arch;
mod graphics;
mod mem;
mod memory;

use crate::graphics::framebuffer::WRITER;

fn kernel_main(boot_info: BootInformation<'_>, mbi_addr: u32, mbi_size: usize) -> ! {
    init();

    let memory_map = boot_info
        .memory_map_tag()
        .expect("No Multiboot2 memory map");

    let fb_tag = match boot_info.framebuffer_tag() {
        Some(Ok(tag)) => tag,
        Some(Err(_)) => panic!("Invalid Multiboot2 framebuffer"),
        None => panic!("No Multiboot2 framebuffer"),
    };

    // MBI is identity-mapped here, so physical == virtual.
    let mbi_start = mbi_addr as u64;
    let mbi_end = mbi_start + mbi_size as u64;

    let mut frame_alloc =
        unsafe { memory::allocator::Multiboot2FrameAllocator::init(memory_map, mbi_start, mbi_end) };
    let mut mapper = unsafe { memory::allocator::init(VirtAddr::new(0x0)) };

    memory::heap::init_heap(&mut mapper, &mut frame_alloc)
        .expect("heap initialization failed");

    let fb_addr = fb_tag.address();
    let fb_width = fb_tag.width() as usize;
    let fb_height = fb_tag.height() as usize;
    let fb_pitch = fb_tag.pitch() as usize;
    let fb_bpp = fb_tag.bpp() as usize;
    let fb_size_bytes = fb_height
        .checked_mul(fb_pitch)
        .expect("framebuffer size overflow");
    assert!(fb_size_bytes > 0, "framebuffer has zero size");
    assert!(
        fb_bpp == 32 || fb_bpp == 24,
        "unsupported framebuffer bpp"
    );

    let start_virt_addr = VirtAddr::new(0x_5555_5555_0000);

    let start_frame =
        PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(fb_addr));
    let end_frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(
        fb_addr + fb_size_bytes as u64 - 1,
    ));
    let frame_range = PhysFrame::range_inclusive(start_frame, end_frame);

    let start_page = Page::<Size4KiB>::containing_address(start_virt_addr);
    let end_page = Page::<Size4KiB>::containing_address(
        start_virt_addr + fb_size_bytes as u64 - 1,
    );
    let page_range = Page::range_inclusive(start_page, end_page);

    assert_eq!(
        page_range.count(),
        frame_range.count(),
        "framebuffer page/frame count mismatch"
    );
    let page_range = Page::range_inclusive(start_page, end_page);

    for (page, frame) in page_range.zip(frame_range) {
        unsafe {
            mapper
                .map_to(
                    page,
                    frame,
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::NO_CACHE,
                    &mut frame_alloc,
                )
                .expect("framebuffer map_to failed")
                .flush();
        }
    }

    let fb_virt_ptr = start_virt_addr.as_mut_ptr::<u8>();

    crate::graphics::framebuffer::init_framebuffer(
        fb_virt_ptr as u64,
        fb_width,
        fb_height,
        fb_pitch,
        fb_bpp,
    );

    println!(
        "BeanieOS: heap {} KiB @ {:#x}, fb {}x{} pitch {} bpp {} @ {:#x} -> {:#x}",
        memory::heap::HEAP_SIZE / 1024,
        memory::heap::HEAP_START,
        fb_width,
        fb_height,
        fb_pitch,
        fb_bpp,
        fb_addr,
        start_virt_addr.as_u64(),
    );

    {
        use alloc::{boxed::Box, vec::Vec};
        let heap_value = Box::new(41u64);
        println!("heap_value at {:p} = {}", heap_value, *heap_value);
        let mut vec = Vec::new();
        for i in 0..50u64 {
            vec.push(i);
        }
        println!("vec len {} at {:p}", vec.len(), vec.as_slice());
    }

    for y in 100..150 {
        for x in 100..150 {
            WRITER.lock().put_pixel(x, y, 0x00FF0000);
        }
    }

    println!("boot ok");

    loop {
        x86_64::instructions::hlt();
    }
}

pub fn init() {
    arch::gdt::init();
    arch::interrupts::init_idt();

    unsafe {
        arch::interrupts::PICS.lock().initialize();
    }

    x86_64::instructions::interrupts::enable();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);

    loop {
        x86_64::instructions::hlt();
    }
}
