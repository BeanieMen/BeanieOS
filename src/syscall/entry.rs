use core::arch::naked_asm;

use crate::arch::gdt::kernel_code_selector;
use crate::arch::interrupts::idt_addr;
use crate::memory::mmu::{USER_END, USER_START};
use crate::syscall::dispatch::dispatch;

pub(crate) const VECTOR: u8 = 0x80;

#[repr(C)]
pub(crate) struct Regs {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
}

const CPU_FRAME_WORDS: usize = 5;

const REGS_WORDS: usize = core::mem::size_of::<Regs>() / 8;

const ENTRY_BLOCK_WORDS: usize = REGS_WORDS + CPU_FRAME_WORDS;

#[unsafe(no_mangle)]extern "C" fn syscall_entry_prepare(regs: *mut u64) -> usize {
    let frame = unsafe { regs.add(REGS_WORDS) };

    let code_selector = unsafe { *frame.add(1) };

    if code_selector & 0x3 == 0 {
        return regs as usize;
    }

    let Some(stack_top) = crate::task::identity::current_kernel_stack_top() else {
        return regs as usize;
    };

    let block = stack_top - ENTRY_BLOCK_WORDS * 8;

    unsafe { core::ptr::copy_nonoverlapping(regs, block as *mut u64, ENTRY_BLOCK_WORDS) };

    block
}

#[unsafe(naked)]
pub unsafe extern "C" fn stub() -> ! {
    naked_asm!(
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov rdi, rsp",
        "call syscall_entry_prepare",
        "mov rsp, rax",
        "mov rdi, rsp",
        "call syscall_c",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "add rsp, 8",
        "mov rax, [rsp - 8]",
        "iretq",
    )
}


#[unsafe(no_mangle)]
pub extern "C" fn syscall_c(regs: *mut Regs) {
    let regs = unsafe { &mut *regs };

    let number = regs.rax;
    let args = [regs.rdi, regs.rsi, regs.rdx, regs.r10, regs.r8, regs.r9];

    let result = super::error::encode(dispatch(number, args));

    crate::kdebug!("syscall {number} -> {result:#x}");

    regs.rax = result;
}

pub(crate) fn install() {
    let addr = stub as usize as u64;
    let selector = u64::from(kernel_code_selector().0);

    let low =
        (addr & 0xffff) | (selector << 16) | (0xee << 40) | (((addr >> 16) & 0xffff) << 48);

    let high = (addr >> 32) as u64;

    let base = idt_addr() + VECTOR as u64 * 16;

    unsafe {
        core::ptr::write_volatile(base as *mut u64, low);
        core::ptr::write_volatile((base + 8) as *mut u64, high);
    }
}

pub(crate) fn copy_from_user(dest: &mut [u8], src: u64) -> Result<usize, ()> {
    let len = dest.len() as u64;

    if src < USER_START || src + len > USER_END {
        return Err(());
    }

    unsafe { core::ptr::copy_nonoverlapping(src as *const u8, dest.as_mut_ptr(), len as usize) };

    Ok(len as usize)
}

pub(crate) fn copy_to_user(dest: u64, src: &[u8]) -> Result<usize, ()> {
    let len = src.len() as u64;

    if dest < USER_START || dest + len > USER_END {
        return Err(());
    }

    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dest as *mut u8, len as usize) };

    Ok(len as usize)
}

const MAX_USER_PATH: usize = 4096;

pub(crate) fn user_path(ptr: u64) -> Result<alloc::vec::Vec<u8>, ()> {
    if ptr < USER_START {
        return Err(());
    }

    let mut path = alloc::vec::Vec::with_capacity(64);

    for offset in 0..MAX_USER_PATH as u64 {
        let addr = ptr + offset;

        if addr >= USER_END {
            return Err(());
        }

        // SAFETY: `addr` is in the user range and reads the byte the pointer names.
        // One byte at a time, so nothing reads past the terminator.
        let byte = unsafe { (addr as *const u8).read() };

        if byte == 0 {
            return Ok(path);
        }

        path.push(byte);
    }

    Err(())
}
