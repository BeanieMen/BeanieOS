use core::arch::naked_asm;

use x86_64::PhysAddr;
use x86_64::structures::paging::{PageTableFlags, PhysFrame};

use crate::arch::gdt::{user_code_selector, user_data_selector};
use crate::memory::allocator::alloc_frame;
use crate::memory::mmu::AddressSpace;
pub(crate) use crate::memory::mmu::{USER_BASE, USER_DATA, USER_STACK_TOP};
use crate::paging::{PAGE_SIZE, align_down, phys_to_direct};

const USER_CODE: &[u8] = &[
    0xb8, 0x08, 0x00, 0x00, 0x00, 0xcd, 0x80, 0xb8, 0x00, 0x00, 0x00, 0x00, 0xcd, 0x80, 0xeb, 0xfe,
];

const SUPERVISOR: PageTableFlags = crate::memory::mmu::KERNEL_RW;

const USER: PageTableFlags = crate::memory::mmu::USER_RW;

#[unsafe(naked)]
pub unsafe extern "C" fn stack_pointer() -> u64 {
    naked_asm!("mov rax, rsp", "ret")
}

#[unsafe(naked)]
pub unsafe extern "C" fn enter_userland(rip: u64, rsp: u64, rflags: u64, ss: u64, cs: u64) -> ! {
    naked_asm!(
        "mov rsp, rsi",
        "sub rsp, 40",
        "mov [rsp], rdi",
        "mov [rsp + 8], r8",
        "mov [rsp + 16], rdx",
        "mov [rsp + 24], rsi",
        "mov [rsp + 32], rcx",
        "iretq",
    )
}

fn page_of(addr: u64) -> PhysFrame {
    PhysFrame::containing_address(PhysAddr::new(align_down(addr)))
}

pub(crate) fn build() -> Option<AddressSpace> {
    let mut space = AddressSpace::new_with_kernel().ok()?;

    let code = alloc_frame()?;
    space.map(USER_BASE, code, USER).ok()?;

    unsafe {
        core::ptr::copy_nonoverlapping(
            USER_CODE.as_ptr(),
            phys_to_direct(code.start_address().as_u64()) as *mut u8,
            USER_CODE.len(),
        )
    };

    let stack = alloc_frame()?;
    space.map(USER_STACK_TOP - PAGE_SIZE, stack, USER).ok()?;

    let data = alloc_frame()?;
    space.map(USER_DATA, data, USER).ok()?;

    Some(space)
}

pub(crate) fn verify_release() -> Result<(), &'static str> {
    use crate::memory::allocator::live_frames;
    use crate::memory::mmu::{USER_BASE, USER_DATA, USER_STACK_TOP};

    let before = live_frames();

    let mut space = AddressSpace::new().map_err(|_| "no frame for a root table")?;

    for addr in [USER_BASE, USER_STACK_TOP - PAGE_SIZE, USER_DATA] {
        space
            .map_fresh(addr, PAGE_SIZE, USER)
            .map_err(|_| "could not map a user page")?;
    }

    let peak = live_frames();

    if peak <= before {
        return Err("mapping allocated nothing, so the test proves nothing");
    }

    drop(space);

    let after = live_frames();

    if after != before {
        return Err("frames leaked");
    }

    Ok(())
}

pub(crate) fn enter(space: &AddressSpace) -> ! {
    space.activate();

    let ss = u64::from(user_data_selector().0) | 0x3;
    let cs = u64::from(user_code_selector().0) | 0x3;

    unsafe { enter_userland(USER_BASE, USER_STACK_TOP, 0x202, ss & 0xffff, cs & 0xffff) }
}
