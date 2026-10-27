use crate::memory::allocator::alloc_frame;
use crate::memory::mmu::{AddressSpace, USER_STACK_TOP};
use crate::paging::phys_to_direct;
use alloc::vec::Vec;
use x86_64::align_down;
use x86_64::structures::paging::PageTableFlags;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_2LSB: u8 = 1;
const EM_X86_64: u16 = 0x3e;
const PT_LOAD: u32 = 1;
const PF_W: u32 = 1 << 1;

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
    pub phdr_vaddr: u64,
    pub phdr_count: u64,
    pub phdr_entry_size: u64,
    pub interp: Option<Vec<u8>>,
    pub base_addr: u64,
}

pub fn load_elf(space: &mut AddressSpace, binary: &[u8]) -> Result<LoadedProcess, &'static str> {
    load_elf_at(space, binary, 0)
}
pub fn load_elf_at(
    space: &mut AddressSpace,
    binary: &[u8],
    default_base: u64,
) -> Result<LoadedProcess, &'static str> {
    if binary.len() < core::mem::size_of::<ElfHeader>() {
        return Err("ELF file too small for header");
    }

    let header = unsafe { core::ptr::read_unaligned(binary.as_ptr() as *const ElfHeader) };
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
    if header.elf_type != 2 && header.elf_type != 3 {
        return Err("only statically linked ET_EXEC binaries supported");
    }
    let base_addr = if header.elf_type == 3 {
        if default_base == 0 {
            0x0040_0000
        } else {
            default_base
        }
    } else {
        0
    };

    let ph_size = header.phentsize as usize;
    let ph_count = header.phnum as usize;
    let ph_table_offset = header.phoff as usize;
    let mut phdr_vaddr = 0u64;
    let mut interp = None;
    
    for i in 0..ph_count {
        let ph_offset = ph_table_offset + i * ph_size;
        if ph_offset + ph_size > binary.len() {
            return Err("program header out of bounds");
        }
        let ph = unsafe { core::ptr::read_unaligned(binary.as_ptr().add(ph_offset) as *const Elf64ProgramHeader) };
        if ph.p_type == 3 {
            let offset = ph.p_offset as usize;
            let filesz = ph.p_filesz as usize;
            if offset + filesz <= binary.len() {
                let mut path = binary[offset..offset + filesz].to_vec();
                if let Some(pos) = path.iter().position(|&b| b==0) {
                    path.truncate(pos);
                }
                interp = Some(path);
            }
            continue
        }
        if ph.p_type == 6 {
            phdr_vaddr = ph.p_vaddr + base_addr;
        }
        if ph.p_type != PT_LOAD {
            continue;
        }
        if phdr_vaddr == 0 {
            let ph_end = ph_table_offset as u64 + (ph_count * ph_size) as u64;
            if ph_end <= ph.p_offset + ph.p_filesz {
                phdr_vaddr = ph.p_vaddr + base_addr + ph_table_offset as u64 - ph.p_offset;
            }
        }
        if ph.p_memsz == 0 {
            continue;
        }

        let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if ph.p_flags & PF_W != 0 {
            flags |= PageTableFlags::WRITABLE;
        }

        let start_vaddr = ph.p_vaddr + base_addr;
        let mem_size = ph.p_memsz;
        let end_vaddr = start_vaddr.checked_add(mem_size).ok_or("program segment address overflow")?;
        if end_vaddr > crate::memory::mmu::USER_END {
            return Err("program segment exceeds userland address space");
        }
        let file_size = ph.p_filesz;
        let file_offset = ph.p_offset as usize;

        if file_offset + file_size as usize > binary.len() {
            return Err("program segment out of bounds");
        }
        let start_page = align_down(start_vaddr, 4096);
        let end_page = align_down(start_vaddr + mem_size.saturating_sub(1), 4096);

        let mut curr_page = start_page;
        let file_end = start_vaddr + file_size;

        while curr_page <= end_page {
            let frame = alloc_frame().ok_or("out of frames")?;
            space
                .map(curr_page, frame, flags)
                .map_err(|_| "failed to map page")?;

            let direct_ptr = phys_to_direct(frame.start_address().as_u64()) as *mut u8;
            let page_end = curr_page + 4096;

            if curr_page < file_end && page_end > start_vaddr {
                let copy_start_vaddr = curr_page.max(start_vaddr);
                let copy_end_vaddr = page_end.min(file_end);
                let copy_len = (copy_end_vaddr - copy_start_vaddr) as usize;

                let page_offset = (copy_start_vaddr - curr_page) as usize;
                let file_data_offset = (copy_start_vaddr - start_vaddr) as usize;

                unsafe {
                    if page_offset > 0 {
                        core::ptr::write_bytes(direct_ptr, 0, page_offset);
                    }
                    core::ptr::copy_nonoverlapping(
                        binary.as_ptr().add(file_offset + file_data_offset),
                        direct_ptr.add(page_offset),
                        copy_len,
                    );
                    let tail_offset = page_offset + copy_len;
                    if tail_offset < 4096 {
                        core::ptr::write_bytes(direct_ptr.add(tail_offset), 0, 4096 - tail_offset);
                    }
                }
            } else {
                unsafe {
                    core::ptr::write_bytes(direct_ptr, 0, 4096);
                }
            }
            curr_page += 4096;
        }
    }

    const STACK_PAGES: u64 = 16;
    let stack_bottom = USER_STACK_TOP - STACK_PAGES * 4096;
    let stack_flags =
        PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE;

    for i in 0..STACK_PAGES {
        let frame = alloc_frame().ok_or("out of frames for stack")?;
        space
            .map(stack_bottom + i * 4096, frame, stack_flags)
            .map_err(|_| "failed to map stack page")?;
        let direct_ptr = phys_to_direct(frame.start_address().as_u64()) as *mut u8;
        unsafe {
            core::ptr::write_bytes(direct_ptr, 0, 4096);
        }
    }

    let user_stack_top = (USER_STACK_TOP - 16) & !0xf;

    Ok(LoadedProcess {
        entry_point: header.entry + base_addr,
        user_stack_top,
        phdr_vaddr,
        phdr_entry_size: ph_size as u64,
        phdr_count: ph_count as u64,
        interp,
        base_addr,
    })
}
