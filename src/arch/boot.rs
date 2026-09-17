use crate::kernel_main;

const MULTIBOOT2_MAGIC: u32 = 0x36d76289;

#[inline(never)]
fn halt() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_entry(magic: u32, mbi_addr: u32) -> ! {
    if magic != MULTIBOOT2_MAGIC {
        halt();
    }

    // Copy out the MBI total_size before handing the borrowed
    // BootInformation to the kernel. The frame allocator must never
    // hand out the MBI pages (see memory::memory::Multiboot2FrameAllocator),
    // otherwise BootInformation would be corrupted as soon as we map
    // the heap / framebuffer at high addresses.
    let mbi_total_size = unsafe { (mbi_addr as *const u32).read() as usize };

    let boot_info = unsafe {
        match multiboot2::BootInformation::load(
            mbi_addr as *const multiboot2::BootInformationHeader,
        ) {
            Ok(info) => info,
            Err(_) => halt(),
        }
    };

    kernel_main(boot_info, mbi_addr, mbi_total_size);
}