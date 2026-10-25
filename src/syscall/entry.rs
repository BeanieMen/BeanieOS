use core::arch::naked_asm;

use x86_64::registers::model_specific;

use crate::arch::gdt::kernel_code_selector;
use crate::arch::interrupts::idt_addr;
use crate::memory::mmu::USER_END;
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
        "pop r15", "pop r14", "pop r13", "pop r12", "pop r11", "pop r10", "pop r9", "pop r8",
        "pop rbp", "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rax", "iretq",
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() -> ! {
    naked_asm!(
        // 1. Swap to kernel GS to access CPU_SCRATCH
        "swapgs",
        "mov gs:[8], rsp", // Save user rsp to CPU_SCRATCH.user_rsp
        "mov rsp, gs:[0]", // Load kernel_stack_top into rsp
        // 2. Build the standard iretq frame
        "push 0x23",   // User SS (selector 4 | 3)
        "push gs:[8]", // User RSP
        "swapgs",      // Restore user GS
        "push r11",    // User RFLAGS (saved by CPU in r11)
        "push 0x2b",   // User CS (selector 5 | 3)
        "push rcx",    // User RIP (saved by CPU in rcx)
        // 3. Save GPRs to match SyscallFrame
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
        "xor eax, eax", // Reset SS to 0 for Ring 0 to prevent #GP on nested interrupt iretq
        "mov ss, eax",
        "sub rsp, 8", // Align stack to 16 bytes for System V ABI call
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

    if number != crate::syscall::numbers::Syscall::RtSigreturn as u64 {
        handle_pending_signals(frame);
    }

    if crate::task::scheduler::is_reschedule_requested() {
        crate::task::scheduler::yield_now();
    }
}

fn handle_pending_signals(frame: &mut SyscallFrame) {
    let Some(proc) = crate::task::process::current() else {
        return;
    };
    let mut p = proc.lock();

    let active = p.pending_signals & !p.blocked_signals;
    if active == 0 {
        return;
    }

    let sig = active.trailing_zeros() as usize + 1;
    let sig_mask_bit = 1u64 << (sig - 1);
    p.pending_signals &= !sig_mask_bit;

    let action = p.sigactions[sig - 1];

    match action.sa_handler {
        crate::task::signal::SIG_IGN => {}
        crate::task::signal::SIG_DFL => {
            match sig {
                17 /* SIGCHLD */ | 18 /* SIGCONT */ | 23 /* SIGURG */ | 28 /* SIGWINCH */ => {}
                _ => {
                    let pid = p.pid;
                    drop(p);
                    crate::task::process::exit(pid, 128 + sig as i32);
                    crate::task::scheduler::exit();
                }
            }
        }
        handler_addr => {
            let saved_mask = p.blocked_signals;
            const UNBLOCKABLE: u64 = (1u64 << (9 - 1)) | (1u64 << (19 - 1));
            p.blocked_signals |= (action.sa_mask & !UNBLOCKABLE) | sig_mask_bit;

            let ctx = crate::task::signal::SignalUserContext {
                regs: frame.regs,
                rip: frame.rip,
                rflags: frame.rflags,
                rsp: frame.rsp,
                mask: saved_mask,
                sig: sig as u64,
            };

            drop(p);

            let ctx_size = core::mem::size_of::<crate::task::signal::SignalUserContext>() as u64;
            let mut new_rsp = (frame.rsp.saturating_sub(128 + ctx_size)) & !0xF;

            let ctx_bytes = unsafe {
                core::slice::from_raw_parts(&ctx as *const _ as *const u8, ctx_size as usize)
            };
            if copy_to_user(new_rsp, ctx_bytes).is_err() {
                crate::task::process::exit(crate::task::identity::current_pid(), 139);
                crate::task::scheduler::exit();
            }

            if action.sa_restorer != 0 {
                new_rsp -= 8;
                let restorer_bytes = action.sa_restorer.to_le_bytes();
                let _ = copy_to_user(new_rsp, &restorer_bytes);
            }

            frame.rsp = new_rsp;
            frame.rip = handler_addr;
            frame.regs.rdi = sig as u64;
        }
    }
}

pub(crate) fn install() {
    use x86_64::VirtAddr;
    use x86_64::registers::model_specific::{Efer, EferFlags, KernelGsBase, LStar, SFMask};
    use x86_64::registers::rflags::RFlags;

    unsafe {
        Efer::update(|efer| *efer |= EferFlags::SYSTEM_CALL_EXTENSIONS);
        LStar::write(VirtAddr::new(syscall_entry as *const () as usize as u64));
        let star_val = (0x08u64 << 32) | (0x18u64 << 48);
        model_specific::Msr::new(0xc0000081).write(star_val);
        SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG);
        KernelGsBase::write(VirtAddr::from_ptr(&raw const crate::arch::gdt::CPU_SCRATCH));
    }
    let addr = stub as *const () as usize as u64;
    let selector = u64::from(kernel_code_selector().0);

    let low = (addr & 0xffff) | (selector << 16) | (0xee << 40) | (((addr >> 16) & 0xffff) << 48);
    let high = addr >> 32;
    let base = idt_addr() + u64::from(VECTOR) * 16;

    unsafe {
        core::ptr::write_volatile(base as *mut u64, low);
        core::ptr::write_volatile((base + 8) as *mut u64, high);
    }
}

pub(crate) fn copy_from_user(dest: &mut [u8], src: u64) -> Result<usize, ()> {
    let len = dest.len() as u64;

    crate::memory::mmu::check_user_range(src, len, false)?;

    unsafe { core::ptr::copy_nonoverlapping(src as *const u8, dest.as_mut_ptr(), len as usize) };

    Ok(len as usize)
}

pub(crate) fn copy_to_user(dest: u64, src: &[u8]) -> Result<usize, ()> {
    let len = src.len() as u64;

    crate::memory::mmu::check_user_range(dest, len, true)?;

    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dest as *mut u8, len as usize) };

    Ok(len as usize)
}

const MAX_USER_PATH: usize = 4096;

pub(crate) fn user_path(ptr: u64) -> Result<alloc::vec::Vec<u8>, ()> {
    if ptr >= USER_END {
        return Err(());
    }

    let mut path = alloc::vec::Vec::with_capacity(64);
    let mut current_page = ptr & !(crate::paging::PAGE_SIZE - 1);
    let root = crate::memory::mmu::current_cr3().start_address().as_u64();
    crate::memory::mmu::try_translate_in(root, current_page, false)?;

    for offset in 0..MAX_USER_PATH as u64 {
        let addr = ptr + offset;

        if addr >= USER_END {
            return Err(());
        }

        let page = addr & !(crate::paging::PAGE_SIZE - 1);
        if page != current_page {
            crate::memory::mmu::try_translate_in(root, page, false)?;
            current_page = page;
        }

        let byte = unsafe { (addr as *const u8).read() };

        if byte == 0 {
            return Ok(path);
        }

        path.push(byte);
    }

    Err(())
}
