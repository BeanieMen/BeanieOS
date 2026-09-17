use crate::{
    framebuffer,
    kernel_main,
};

const MULTIBOOT2_MAGIC: u32 = 0x36d76289;

#[repr(C)]
pub struct MultibootInfo {
    pub total_size: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct Multiboot2FramebufferTag {
    pub typ: u32,
    pub size: u32,
    pub framebuffer_addr: u64,
    pub framebuffer_pitch: u32,
    pub framebuffer_width: u32,
    pub framebuffer_height: u32,
    pub framebuffer_bpp: u8,
    pub framebuffer_type: u8,
    pub reserved: u16,
}

#[repr(C)]
pub struct Multiboot2MemoryMap {
    pub typ: u32,
    pub size: u32,
    pub entry_size: u32,
    pub entry_version: u32,
}

impl Multiboot2MemoryMap {
    pub fn entries(&self) -> MemoryMapIter {
        let entries_start = self as *const Self as usize + 16;
        let entries_end = self as *const Self as usize + self.size as usize;

        MemoryMapIter {
            current: entries_start,
            end: entries_end,
            entry_size: self.entry_size as usize,
        }
    }
}

pub struct MemoryMapIter {
    current: usize,
    end: usize,
    entry_size: usize,
}

impl Iterator for MemoryMapIter {
    type Item = &'static MemoryMapEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current + self.entry_size > self.end {
            return None;
        }

        let entry = unsafe {
            &*(self.current as *const MemoryMapEntry)
        };

        self.current += self.entry_size;

        Some(entry)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_entry(magic: u32, mbi_addr: u32) -> ! {
    if magic != MULTIBOOT2_MAGIC {
        panic!("Invalid Multiboot2 magic: {:#x}", magic);
    }

    let mbi = unsafe {
        &*(mbi_addr as *const MultibootInfo)
    };

    let mut memory_map: Option<&'static Multiboot2MemoryMap> = None;
    let mut fb_tag: Option<&'static Multiboot2FramebufferTag> = None;

    let mut tag_addr = mbi_addr as usize + 8;
    let end_addr = mbi_addr as usize + mbi.total_size as usize;

    while tag_addr < end_addr {
        let tag_type = unsafe {
            *(tag_addr as *const u32)
        };

        let tag_size = unsafe {
            *((tag_addr + 4) as *const u32)
        };

        if tag_type == 0 {
            break;
        }

        let next_tag =
            (tag_addr + tag_size as usize + 7) & !7;

        match tag_type {
            // Memory map
            6 => {
                let tag = unsafe {
                    &*(tag_addr as *const Multiboot2MemoryMap)
                };

                memory_map = Some(tag);
            }

            // Framebuffer
            8 => {
                let tag = unsafe {
                    &*(tag_addr as *const Multiboot2FramebufferTag)
                };

                fb_tag = Some(tag);

                framebuffer::init_framebuffer(
                    tag.framebuffer_addr,
                    tag.framebuffer_width as usize,
                    tag.framebuffer_height as usize,
                    tag.framebuffer_pitch as usize,
                    tag.framebuffer_bpp as usize,
                );
            }

            _ => {}
        }

        tag_addr = next_tag;
    }

    kernel_main(memory_map, fb_tag);
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryMapEntry {
    pub base_addr: u64,
    pub length: u64,
    pub entry_type: u32,
    pub reserved: u32,
}
