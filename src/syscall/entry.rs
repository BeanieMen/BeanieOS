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
        "mov [rsp], rax",
        "add rsp, 8",
        "iretq",
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn syscall_c(regs: *mut Regs) {
    use core::sync::atomic::{AtomicU64, Ordering};

    use x86_64::PhysAddr;
    use x86_64::registers::control::{Cr3, Cr3Flags};
    use x86_64::structures::paging::PhysFrame;

    static USER_CR3: AtomicU64 = AtomicU64::new(0);

    let regs = unsafe { &mut *regs };

    let number = regs.rax;
    let args = [regs.rdi, regs.rsi, regs.rdx, regs.r10, regs.r8, regs.r9];

    let kernel = crate::memory::mmu::kernel_cr3();
    let current = crate::memory::mmu::current_cr3();
    let switched = current != kernel;

    if switched {
        USER_CR3.store(current.start_address().as_u64(), Ordering::Release);
        crate::memory::mmu::switch_cr3(kernel);
    }

    let result = super::error::encode(dispatch(number, args));

    crate::kdebug!("syscall {number} -> {result:#x}");

    regs.rax = result;

    if switched {
        let back = PhysFrame::containing_address(PhysAddr::new(USER_CR3.load(Ordering::Acquire)));
        crate::memory::mmu::switch_cr3(back);
    }
}

pub(crate) fn install() {
    let addr = stub as usize as u64;
    let selector = u64::from(kernel_code_selector().0);

    let entry = (addr & 0xffff) | (selector << 16) | (0xee << 40) | (((addr >> 16) & 0xffff) << 48);

    let base = idt_addr() + VECTOR as u64 * 16;

    unsafe { core::ptr::write_volatile(base as *mut u64, entry) };
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

// Longest path accepted from user space, terminator included. Not optional:
// reading a user pointer until it lands on a zero byte hands the length of a
// kernel read to whatever the caller mapped.
const MAX_USER_PATH: usize = 4096;

// Copies a NUL-terminated path into an owned buffer. Owned rather than a
// `&'static str` built straight from the raw pointer: the process can exit or
// unmap the page, and the string outlives both. The bound is applied per byte,
// since copying a whole MAX_USER_PATH window would read past the caller's
// mapping, which is the fault this is meant to survive.
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

        // SAFETY: `addr` is within the user range and this reads exactly the byte the
        // caller's pointer refers to. One byte at a time, so no read past the
        // terminator once one is found.
        let byte = unsafe { (addr as *const u8).read() };

        if byte == 0 {
            return Ok(path);
        }

        path.push(byte);
    }

    Err(())
}
