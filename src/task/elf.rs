use crate::memory::allocator::alloc_frame;
use crate::memory::mmu::{AddressSpace, USER_STACK_TOP};
use crate::paging::phys_to_direct;
use x86_64::align_down;
use x86_64::structures::paging::PageTableFlags;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_2LSB: u8 = 1;
const EM_X86_64: u16 = 0x3e;
const PT_LOAD: u32 = 1;

const PF_X: u32 = 1 << 0;
const PF_W: u32 = 1 << 1;
const PF_R: u32 = 1 << 2;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ElfHeader {
    pub ident: [u8; 16],
    pub elf_type: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

pub struct LoadedProcess {
    pub entry_point: u64,
    pub user_stack_top: u64,
}

pub fn load_elf(space: &mut AddressSpace, binary: &[u8]) -> Result<LoadedProcess, &'static str> {
    if binary.len() < core::mem::size_of::<ElfHeader>() {
        return Err("ELF file too small for header");
    }

    // validate header
    let header = unsafe { &*(binary.as_ptr() as *const ElfHeader) };
    if header.ident[0..4] != ELF_MAGIC {
        return Err("invalid ELF magic");
    }
    if header.ident[4] != ELF_CLASS_64 {
        return Err("unsupported ELF class");
    }
    if header.ident[5] != ELF_DATA_2LSB {
        return Err("unsupported ELF data encoding");
    }
    if header.machine != EM_X86_64 {
        return Err("unsupported machine type");
    }

    let ph_size = header.phentsize as usize;
    let ph_count = header.phnum as usize;
    let ph_table_offset = header.phoff as usize;

    for i in 0..ph_count {
        let ph_offset = ph_table_offset + i * ph_size;
        if ph_offset + ph_size > binary.len() {
            return Err("program header out of bounds");
        }
        let ph = unsafe { &*(binary.as_ptr().add(ph_offset) as *const Elf64ProgramHeader) };
        if ph.p_type != PT_LOAD {
            continue;
        }
        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if ph.p_flags & PF_W != 0 {
            flags |= PageTableFlags::WRITABLE;
        }

        let start_vaddr = ph.p_vaddr;
        let mem_size = ph.p_memsz;
        let file_size = ph.p_filesz;
        let file_offset = ph.p_offset as usize;

        if file_offset + file_size as usize > binary.len() {
            return Err("program segment out of bounds");
        }
        let start_page = align_down(start_vaddr, 4096);
        let end_page = align_down(start_vaddr + mem_size.saturating_sub(1), 4096);

        let mut curr_page = start_page;

        while curr_page <= end_page {
            let frame = alloc_frame().ok_or("out of frames")?;
            space
                .map(curr_page, frame, flags)
                .map_err(|_| "failed to map page")?;

            let direct_ptr = phys_to_direct(frame.start_address().as_u64()) as *mut u8;
            unsafe {
                core::ptr::write_bytes(direct_ptr, 0, 4096);
            }
            curr_page += 4096;
        }

        if file_size > 0 {
            let src = &binary[file_offset..file_offset + file_size as usize];
            let mut bytes_copied = 0usize;

            while bytes_copied < file_size as usize {
                let curr_vaddr = start_vaddr + bytes_copied as u64;
                let page_offset = (curr_vaddr % 4096) as usize;
                let chunk_len =
                    core::cmp::min(4096 - page_offset, file_size as usize - bytes_copied);

                let frame = space.translate(curr_vaddr);
                let dest = phys_to_direct(frame.start_address().as_u64()) as *mut u8;

                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src.as_ptr().add(bytes_copied),
                        dest.add(page_offset),
                        chunk_len,
                    );
                }
                bytes_copied += chunk_len;
            }
        }
    }

    const STACK_PAGES: u64 = 4; // 4*4096 16kib 
    let stack_bottom = USER_STACK_TOP - STACK_PAGES * 4096;
    let stack_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE;

    for i in 0..STACK_PAGES {
        let frame = alloc_frame().ok_or("out of frames for stack")?;
        space.map(stack_bottom + i * 4096, frame, stack_flags)
            .map_err(|_| "failed to map stack page")?;
        let direct_ptr = phys_to_direct(frame.start_address().as_u64()) as *mut u8;
        unsafe {
            core::ptr::write_bytes(direct_ptr, 0, 4096);
        }
    }

    let user_stack_top = (USER_STACK_TOP - 16) & !0xf; // align to 16 bytes

    Ok(LoadedProcess {
        entry_point: header.entry,
        user_stack_top,
    })
}
