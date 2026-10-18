use core::{arch::naked_asm, cell::UnsafeCell};

use alloc::{boxed::Box, string::String, vec};

use crate::task::identity::{
    Priority, ProcessId, STACK_SIZE, TIMESLICE_TICKS, ThreadId, ThreadState,
};

#[allow(dead_code)]
const RBP_WORD: usize = 0;
#[allow(dead_code)]
const RBX_WORD: usize = 1;
#[allow(dead_code)]
const R12_WORD: usize = 2;
#[allow(dead_code)]
const R13_WORD: usize = 3;
#[allow(dead_code)]
const R14_WORD: usize = 4;
#[allow(dead_code)]
const R15_WORD: usize = 5;
const RFLAGS_WORD: usize = 6;
const RIP_WORD: usize = 7;
const RETURN_WORD: usize = 8;

const FRAME_WORDS: usize = RETURN_WORD + 1;
const FRAME_BYTES: usize = FRAME_WORDS * 8;

const RFLAGS_IF: usize = 0x202;

pub(crate) struct Thread {
    pub id: ThreadId,
    pub pid: ProcessId,
    #[allow(dead_code)]
    pub name: String,
    pub state: ThreadState,
    pub priority: Priority,
    #[allow(dead_code)]
    pub stack: Option<Box<[u8]>>,
    pub fs_base: u64,
    pub timeslice: u64,
    rsp: UnsafeCell<usize>,
}

impl Thread {
    pub(crate) fn boot() -> Self {
        Thread {
            id: ThreadId::MAIN,
            pid: ProcessId::KERNEL,
            name: String::from("main"),
            state: ThreadState::Running,
            priority: Priority::Normal,
            rsp: UnsafeCell::new(0),
            stack: None,
            fs_base: 0,
            timeslice: TIMESLICE_TICKS,
        }
    }

    pub fn new(pid: ProcessId, name: &str, entry: extern "C" fn(), priority: Priority) -> Self {
        let stack = vec![0u8; STACK_SIZE].into_boxed_slice();
        let stack_top = (stack.as_ptr() as usize + STACK_SIZE) & !0xf;
        let sp = stack_top - FRAME_BYTES;

        build_initial_frame(sp, entry);

        Thread {
            id: ThreadId::new(),
            pid,
            name: String::from(name),
            state: ThreadState::Ready,
            priority,
            rsp: UnsafeCell::new(sp),
            stack: Some(stack),
            fs_base: 0,
            timeslice: TIMESLICE_TICKS,
        }
    }

    pub(crate) fn new_forked(
        pid: ProcessId,
        name: &str,
        priority: Priority,
        parent_frame: &crate::syscall::entry::SyscallFrame,
    ) -> Self {
        let stack = vec![0u8; STACK_SIZE].into_boxed_slice();
        let stack_top = (stack.as_ptr() as usize + STACK_SIZE) & !0xf;

        let frame_size = core::mem::size_of::<crate::syscall::entry::SyscallFrame>();
        let syscall_frame_sp = stack_top - frame_size;

        let mut child_frame = *parent_frame;
        child_frame.regs.rax = 0;

        unsafe {
            core::ptr::copy_nonoverlapping(
                &child_frame as *const crate::syscall::entry::SyscallFrame,
                syscall_frame_sp as *mut crate::syscall::entry::SyscallFrame,
                1,
            );
        }

        const SWITCH_WORDS: usize = 8;
        const SWITCH_BYTES: usize = SWITCH_WORDS * 8;
        let sp = syscall_frame_sp - SWITCH_BYTES;

        let frame = sp as *mut usize;
        unsafe {
            core::ptr::write_bytes(frame, 0, SWITCH_WORDS);
            frame.add(6).write(RFLAGS_IF);
            frame.add(7).write(crate::syscall::entry::fork_return_trampoline as *const () as usize);
        }

        Thread {
            id: ThreadId::new(),
            pid,
            name: String::from(name),
            state: ThreadState::Ready,
            priority,
            rsp: UnsafeCell::new(sp),
            stack: Some(stack),
            fs_base: x86_64::registers::model_specific::FsBase::read().as_u64(),
            timeslice: TIMESLICE_TICKS,
        }
    }

    pub(crate) fn saved_rsp(&self) -> usize {
        unsafe { *self.rsp.get() }
    }

    pub(crate) fn rsp_slot(&self) -> *mut usize {
        self.rsp.get()
    }

    pub(crate) fn requeue(&mut self) {
        self.state = ThreadState::Ready;
        self.timeslice = TIMESLICE_TICKS;
    }

    pub(crate) fn begin_running(&mut self) {
        self.state = ThreadState::Running;
        self.timeslice = TIMESLICE_TICKS;
    }

    pub(crate) fn kernel_stack_top(&self) -> Option<usize> {
        self.stack
            .as_ref()
            .map(|stack| (stack.as_ptr() as usize + stack.len()) & !0xf)
    }
}

fn build_initial_frame(sp: usize, entry: extern "C" fn()) {
    debug_assert_eq!(sp % 16, 8);
    debug_assert_eq!((sp + RIP_WORD * 8) % 16, 0);

    let frame = sp as *mut usize;

    unsafe {
        core::ptr::write_bytes(frame, 0, FRAME_WORDS);

        frame.add(RFLAGS_WORD).write(RFLAGS_IF);
        frame.add(RIP_WORD).write(entry as usize);

        let exit: fn() -> ! = thread_trampoline_exit;
        frame.add(RETURN_WORD).write(exit as usize);
    }
}

fn thread_trampoline_exit() -> ! {
    crate::task::scheduler::exit()
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn switch_context(old_rsp: *mut usize, new_rsp: usize) {
    naked_asm!(
        "pushfq",
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push rbx",
        "push rbp",
        "mov [rdi], rsp",
        "mov rsp, rsi",
        "pop rbp",
        "pop rbx",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        "popfq",
        "ret",
    );
}
