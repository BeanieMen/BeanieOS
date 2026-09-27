use core::mem::size_of;

use crate::kernel_main;

// check if bootloader is mb2 compat
const MULTIBOOT2_MAGIC: u32 = 0x36d76289;

// tell we are mb2 compat
const MB_MAGIC: u32 = 0xe85250d6;
const MB_ARCH: u32 = 0;

#[repr(C, align(8))]
struct InfoRequestTag {
    typ: u16,
    flags: u16,
    size: u32,
    requests: [u32; 6],
}

#[repr(C)]
struct FramebufferRequestTag {
    typ: u16,
    flags: u16,
    size: u32,
    width: u32,
    height: u32,
    depth: u32,
}

#[repr(C)]
struct EndTag {
    typ: u16,
    flags: u16,
    size: u32,
}

#[repr(C, align(8))]
struct MultibootHeader {
    magic: u32,
    architecture: u32,
    header_length: u32,
    checksum: u32,
    info_request: InfoRequestTag,
    framebuffer_request: FramebufferRequestTag,
    _pad: u32,
    end: EndTag,
}

const HEADER_LENGTH: u32 = size_of::<MultibootHeader>() as u32;
const _: () = assert!(
    HEADER_LENGTH == 80,
    "multiboot header layout changed, recheck tag sizes/padding"
);

#[used]
#[unsafe(link_section = ".multiboot_header")]
static MULTIBOOT_HEADER: MultibootHeader = MultibootHeader {
    magic: MB_MAGIC,
    architecture: MB_ARCH,
    header_length: HEADER_LENGTH,
    checksum: MB_MAGIC
        .wrapping_add(MB_ARCH)
        .wrapping_add(HEADER_LENGTH)
        .wrapping_neg(),
    info_request: InfoRequestTag {
        typ: 1,
        flags: 0,
        size: 32,
        // request memory map, boot device, command line, modules, rsdp v1/v2, framebuffer
        requests: [6, 8, 9, 1, 14, 15],
    },
    framebuffer_request: FramebufferRequestTag {
        typ: 5,
        flags: 0,
        size: 20,
        width: 1024,
        height: 768,
        depth: 32,
    },
    _pad: 0,
    end: EndTag {
        typ: 0,
        flags: 0,
        size: 8,
    },
};

const HUGE_PAGE_FLAGS: u64 = 0x83; // present | writable | huge bit

// 1 p4 -> 1 p3 -> 8 p2 -> 1gib pages (8gb addressable)
#[repr(align(4096))]
#[allow(dead_code)]
pub struct AlignedPageTable([u64; 512]);

#[repr(align(4096))]
#[allow(dead_code)]
pub struct AlignedP2Tables([[u64; 512]; 8]);

#[unsafe(no_mangle)]
pub static mut P2_TABLES: AlignedP2Tables = AlignedP2Tables({
    let mut tables = [[0u64; 512]; 8];
    let mut k = 0;
    while k < 8 {
        let mut j = 0;
        while j < 512 {
            tables[k][j] = ((k * 512 + j) as u64 * 0x20_0000) | HUGE_PAGE_FLAGS;
            j += 1;
        }
        k += 1;
    }
    tables
});

#[unsafe(no_mangle)]
pub static mut P3_TABLE: AlignedPageTable = AlignedPageTable([0; 512]);

#[unsafe(no_mangle)]
pub static mut P4_TABLE: AlignedPageTable = AlignedPageTable([0; 512]);

#[repr(align(16))]
#[allow(dead_code)]
pub struct BootStack([u8; 65536]);

#[unsafe(no_mangle)]
pub static mut BOOT_STACK: BootStack = BootStack([0; 65536]);

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
