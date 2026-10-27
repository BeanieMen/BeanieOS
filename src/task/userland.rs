use core::arch::naked_asm;

use x86_64::structures::paging::PageTableFlags;

use crate::arch::gdt::{user_code_selector, user_data_selector};
use crate::memory::allocator::alloc_frame;
use crate::memory::mmu::AddressSpace;
pub(crate) use crate::memory::mmu::{USER_BASE, USER_DATA, USER_STACK_TOP};
use crate::paging::{PAGE_SIZE, phys_to_direct};

const USER_CODE: &[u8] = &[
    0xbf, 0x00, 0x00, 0x50, 0x00, 0xbe, 0x00, 0x00, 0x00, 0x00, 0x31, 0xd2, 0xb8, 0x03, 0x00, 0x00,
    0x00, 0xcd, 0x80, 0x49, 0x89, 0xc4, 0x44, 0x89, 0xe7, 0xbe, 0x00, 0x02, 0x50, 0x00, 0xba, 0x00,
    0x02, 0x00, 0x00, 0xb8, 0x02, 0x00, 0x00, 0x00, 0xcd, 0x80, 0x49, 0x89, 0xc5, 0x44, 0x89, 0xe7,
    0xb8, 0x04, 0x00, 0x00, 0x00, 0xcd, 0x80, 0xbf, 0x40, 0x00, 0x50, 0x00, 0xbe, 0x01, 0x00, 0x00,
    0x00, 0x31, 0xd2, 0xb8, 0x03, 0x00, 0x00, 0x00, 0xcd, 0x80, 0x49, 0x89, 0xc4, 0x44, 0x89, 0xe7,
    0xbe, 0x00, 0x01, 0x50, 0x00, 0xba, 0xb0, 0x04, 0x00, 0x00, 0xb8, 0x01, 0x00, 0x00, 0x00, 0xcd,
    0x80, 0x31, 0xff, 0xb8, 0x00, 0x00, 0x00, 0x00, 0xcd, 0x80, 0xeb, 0xfe,
];

const PATH: &[u8] = b"/TEST.TXT\0";
const OUT_PATH: &[u8] = b"/OUT.TXT\0";
const MARKER: &[u8] = b"RING3 WROTE THIS OVER AHCI\n";
const LONG_LEN: usize = 1200;

const PATH_OFF: usize = 0x00;
const OUT_PATH_OFF: usize = 0x40;
const MARKER_OFF: usize = 0x100;

const USER: PageTableFlags = crate::memory::mmu::USER_RW;

#[unsafe(naked)]
pub unsafe extern "C" fn enter_userland(rip: u64, rsp: u64, rflags: u64, ss: u64, cs: u64) -> ! {
    // SAFETY: Construct the 5-quadword iretq frame (SS, RSP, RFLAGS, CS, RIP) on the
    // supervisor stack and execute iretq to drop to Ring 3 and switch to user RSP.
    naked_asm!(
        "push rcx", // SS
        "push rsi", // RSP
        "push rdx", // RFLAGS
        "push r8",  // CS
        "push rdi", // RIP
        "iretq",
    )
}

pub(crate) fn build() -> Option<AddressSpace> {
    if USER_CODE.len() > PAGE_SIZE as usize {
        return None;
    }

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

    let page = phys_to_direct(data.start_address().as_u64()) as *mut u8;

    unsafe {
        core::ptr::copy_nonoverlapping(PATH.as_ptr(), page.add(PATH_OFF), PATH.len());
        core::ptr::copy_nonoverlapping(OUT_PATH.as_ptr(), page.add(OUT_PATH_OFF), OUT_PATH.len());
        core::ptr::copy_nonoverlapping(MARKER.as_ptr(), page.add(MARKER_OFF), MARKER.len());

        let long = page.add(MARKER_OFF);

        for i in 0..LONG_LEN {
            *long.add(i) = b'A' + (i % 26) as u8;
        }
    }

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
