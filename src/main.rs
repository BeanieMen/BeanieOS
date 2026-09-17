#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use x86_64::{
    PhysAddr,
    VirtAddr,
    structures::paging::{
        Mapper,
        Page,
        PageTableFlags,
        PhysFrame,
        Size4KiB,
    },
};

extern crate alloc;

mod allocator;
mod gdt;
mod interrupts;
mod mem;
mod memory;
mod framebuffer;
mod boot;

pub use boot::{
    MemoryMapEntry,
    Multiboot2FramebufferTag,
    Multiboot2MemoryMap,
};

use crate::framebuffer::WRITER;

fn kernel_main(
    memory_map: Option<&'static Multiboot2MemoryMap>,
    fb_tag: Option<&'static Multiboot2FramebufferTag>,
) -> ! {
    init();

    let memory_map =
        memory_map.expect("No Multiboot2 memory map");

    let Some(fb_tag) = fb_tag else {
        panic!("No Multiboot2 framebuffer");
    };

    let mut frame_alloc =
        unsafe {
            memory::Multiboot2FrameAllocator::init(memory_map)
        };

    let mut mapper =
        unsafe {
            memory::init(VirtAddr::new(0x0))
        };

    let fb_size_bytes =
        (fb_tag.framebuffer_height as usize)
            * (fb_tag.framebuffer_pitch as usize);

    let start_virt_addr =
        VirtAddr::new(0x_5555_5555_0000);

    let start_frame =
        PhysFrame::<Size4KiB>::containing_address(
            PhysAddr::new(fb_tag.framebuffer_addr),
        );

    let end_frame =
        PhysFrame::<Size4KiB>::containing_address(
            PhysAddr::new(
                fb_tag.framebuffer_addr
                    + fb_size_bytes as u64
                    - 1,
            ),
        );

    let frame_range =
        PhysFrame::range_inclusive(
            start_frame,
            end_frame,
        );

    let start_page =
        Page::<Size4KiB>::containing_address(
            start_virt_addr,
        );

    let end_page =
        Page::<Size4KiB>::containing_address(
            start_virt_addr
                + fb_size_bytes as u64
                - 1,
        );

    let page_range =
        Page::range_inclusive(
            start_page,
            end_page,
        );

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
                .unwrap()
                .flush();
        }
    }

    let fb_ptr =
        start_virt_addr.as_mut_ptr::<u8>();

    let pitch =
        fb_tag.framebuffer_pitch as usize;

    let bpp =
        fb_tag.framebuffer_bpp as usize;

    // 50x50 red box
    for y in 0..50 {
        for x in 0..50 {
            unsafe {
                WRITER.lock().put_pixel(
    
                    x,
                    y,
                    0x00FF0000,
                );
            }
        }
    }

    loop {
        x86_64::instructions::hlt();
    }
}

pub fn init() {
    gdt::init();
    interrupts::init_idt();

    unsafe {
        interrupts::PICS.lock().initialize();
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