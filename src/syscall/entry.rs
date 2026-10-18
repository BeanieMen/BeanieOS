use core::arch::naked_asm;

use crate::arch::gdt::kernel_code_selector;
use crate::arch::interrupts::idt_addr;
use crate::memory::mmu::{USER_END, USER_START};
use crate::syscall::dispatch::dispatch;

pub(crate) const VECTOR: u8 = 0x80;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct SyscallFrame {
    pub regs: Regs,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[unsafe(naked)]
pub unsafe extern "C" fn fork_return_trampoline() -> ! {
    naked_asm!(
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
        "pop rax",
        "iretq",
    );
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
        "sub rsp, 8",
        "lea rdi, [rsp + 8]",
        "call syscall_c",
        "add rsp, 8",
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
        "pop rax",
        "iretq",
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn syscall_c(frame: *mut SyscallFrame) {
    let frame = unsafe { &mut *frame };

    let number = frame.regs.rax;
    let args = [
        frame.regs.rdi,
        frame.regs.rsi,
        frame.regs.rdx,
        frame.regs.r10,
        frame.regs.r8,
        frame.regs.r9,
    ];

    let result = super::error::encode(dispatch(number, args, frame));

    crate::kdebug!("syscall {number} -> {result:#x}");

    frame.regs.rax = result;
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
