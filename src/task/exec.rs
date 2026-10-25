use alloc::vec::Vec;

use crate::arch::interrupts::vectors::ticks;
use crate::memory::mmu::AddressSpace;
use crate::paging::{PAGE_SIZE, align_down, phys_to_direct};
use crate::task::elf::{LoadedProcess, load_elf};
use crate::task::{identity, process};

const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_FLAGS: u64 = 8;
const AT_ENTRY: u64 = 9;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_HWCAP: u64 = 16;
const AT_CLKTCK: u64 = 17;
const AT_SECURE: u64 = 23;
const AT_RANDOM: u64 = 25;
const AT_EXECFN: u64 = 31;

const RANDOM_LEN: u64 = 16;
const READ_CHUNK: usize = 64 * 1024;
const MAX_IMAGE: usize = 32 * 1024 * 1024;

pub(crate) fn execve(path: &[u8], argv: &[Vec<u8>], envp: &[Vec<u8>]) -> Result<!, &'static str> {
    let Some(binary) = read_program(path) else {
        return Err("could not read program");
    };

    let mut space = AddressSpace::new_with_kernel().map_err(|_| "no address space")?;

    let loaded = load_elf(&mut space, &binary)?;

    let stack = build_stack(&mut space, &loaded, path, argv, envp)?;

    let previous = space_of_current();

    process::attach_space(identity::current_pid(), space.root);

    space.activate();

    let ss = u64::from(crate::arch::gdt::user_data_selector().0) | 0x3;
    let cs = u64::from(crate::arch::gdt::user_code_selector().0) | 0x3;

    // Leaked on purpose: dropping it here would free the page tables the CPU is
    // still walking. The scheduler has to reclaim it once this thread is gone.
    core::mem::forget(previous);

    let default_fpu = crate::task::thread::FxsaveArea::default_state();
    unsafe {
        core::arch::asm!(
            "fxrstor64 [{0}]",
            in(reg) default_fpu.data.as_ptr(),
            options(nostack, preserves_flags)
        );
        crate::task::userland::enter_userland(
            loaded.entry_point,
            stack,
            0x202,
            ss & 0xffff,
            cs & 0xffff,
        )
    }
}

fn read_program(path: &[u8]) -> Option<Vec<u8>> {
    process::with_current_fds(|fds| {
        let fd = fds.open_read(path).ok()?;

        let mut image = Vec::new();
        let mut chunk = alloc::vec![0u8; READ_CHUNK];

        loop {
            let n = fds.read(fd, &mut chunk).ok()?;

            if n == 0 {
                break;
            }

            if image.len() + n > MAX_IMAGE {
                let _ = fds.close(fd);
                return None;
            }

            image.extend_from_slice(&chunk[..n]);
        }

        let _ = fds.close(fd);

        if image.is_empty() {
            return None;
        }

        Some(image)
    })
    .flatten()
}

fn space_of_current() -> Option<AddressSpace> {
    process::take_space(identity::current_pid())
}

fn build_stack(
    space: &mut AddressSpace,
    loaded: &LoadedProcess,
    path: &[u8],
    argv: &[Vec<u8>],
    envp: &[Vec<u8>],
) -> Result<u64, &'static str> {
    let total_payload: usize = argv.iter().map(|a| a.len() + 1).sum::<usize>()
        + envp.iter().map(|e| e.len() + 1).sum::<usize>()
        + path.len()
        + 1;
    if total_payload > 32 * 1024 {
        return Err("argument list too long");
    }

    let mut at = align_down(loaded.user_stack_top);

    at -= RANDOM_LEN;
    let random_at = at;

    write_user(space, at, &random_bytes())?;

    let execfn = push_string(space, &mut at, path)?;

    let mut envp_ptrs = Vec::with_capacity(envp.len());
    for env in envp {
        envp_ptrs.push(push_string(space, &mut at, env)?);
    }

    let mut argv_ptrs = Vec::with_capacity(argv.len());
    for arg in argv {
        argv_ptrs.push(push_string(space, &mut at, arg)?);
    }

    let auxv = [
        (AT_PHDR, loaded.phdr_vaddr),
        (AT_PHENT, loaded.phdr_entry_size),
        (AT_PHNUM, loaded.phdr_count),
        (AT_PAGESZ, PAGE_SIZE),
        (AT_BASE, 0),
        (AT_FLAGS, 0),
        (AT_ENTRY, loaded.entry_point),
        (AT_UID, 0),
        (AT_EUID, 0),
        (AT_GID, 0),
        (AT_EGID, 0),
        (AT_HWCAP, 0),
        (AT_CLKTCK, 100),
        (AT_SECURE, 0),
        (AT_RANDOM, random_at),
        (AT_EXECFN, execfn),
    ];

    let total_words = 1 + (argv_ptrs.len() + 1) + (envp_ptrs.len() + 1) + (auxv.len() + 1) * 2;
    if !total_words.is_multiple_of(2) {
        at -= 8;
        write_user(space, at, &0u64.to_le_bytes())?;
    }

    at -= (auxv.len() as u64 + 1) * 16;
    for (index, (key, value)) in auxv.iter().enumerate() {
        let slot = at + index as u64 * 16;
        write_user(space, slot, &key.to_le_bytes())?;
        write_user(space, slot + 8, &value.to_le_bytes())?;
    }

    let terminator = at + auxv.len() as u64 * 16;
    write_user(space, terminator, &AT_NULL.to_le_bytes())?;
    write_user(space, terminator + 8, &0u64.to_le_bytes())?;

    at -= (envp_ptrs.len() as u64 + 1) * 8;
    for (index, ptr) in envp_ptrs.iter().enumerate() {
        write_user(space, at + index as u64 * 8, &ptr.to_le_bytes())?;
    }
    write_user(space, at + envp_ptrs.len() as u64 * 8, &0u64.to_le_bytes())?;

    at -= (argv_ptrs.len() as u64 + 1) * 8;
    for (index, ptr) in argv_ptrs.iter().enumerate() {
        write_user(space, at + index as u64 * 8, &ptr.to_le_bytes())?;
    }
    write_user(space, at + argv_ptrs.len() as u64 * 8, &0u64.to_le_bytes())?;

    at -= 8;
    write_user(space, at, &(argv_ptrs.len() as u64).to_le_bytes())?;

    Ok(at)
}

fn push_string(space: &mut AddressSpace, at: &mut u64, bytes: &[u8]) -> Result<u64, &'static str> {
    let start = (*at - bytes.len() as u64 - 1) & !0xf;

    write_user(space, start, bytes)?;
    write_user(space, start + bytes.len() as u64, &[0])?;

    *at = start;

    Ok(start)
}

fn write_user(space: &AddressSpace, addr: u64, bytes: &[u8]) -> Result<(), &'static str> {
    let mut done = 0usize;

    while done < bytes.len() {
        let at = addr + done as u64;

        let frame = space.translate(at);

        let base = phys_to_direct(frame.start_address().as_u64()) as *mut u8;
        let offset = (at % PAGE_SIZE) as usize;

        let chunk = core::cmp::min(PAGE_SIZE as usize - offset, bytes.len() - done);

        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr().add(done), base.add(offset), chunk);
        }

        done += chunk;
    }

    Ok(())
}

fn random_bytes() -> [u8; RANDOM_LEN as usize] {
    let mut out = [0u8; RANDOM_LEN as usize];
    let mut state = ticks() ^ 0x9e37_79b9_7f4a_7c15;

    for slot in &mut out {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *slot = state as u8;
    }

    out
}
