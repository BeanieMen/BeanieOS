# Phase 9: Process Management & Multitasking Scheduler

## 🌟 High-Level Overview
A computer might only have one or a few CPU cores, yet you can listen to music, browse the web, and type in a terminal all at the same time. How is this possible?

The secret is **Multitasking**. The operating system switches between different tasks hundreds of times every second—so fast that to human eyes, everything appears to run simultaneously.

In this phase, we build:
1.  **Process Management (`ProcessManager`):** The bookkeeping system that tracks programs, PIDs, parent-child relationships, and process statuses.
2.  **Thread Abstraction (`Thread`):** Each thread gets its own private **64 KiB stack** in memory.
3.  **The Assembly Context Switcher (`switch_context`):** The ultimate low-level sleight of hand. By swapping a single CPU register (`%rsp`), the CPU instantly teleports from one running function to another!
4.  **A Multi-Queue Priority Scheduler:** Organizes tasks into High, Normal, Low, and Idle queues.

---

## 📖 Layman's Glossary: Jargon Demystified

*   **Process vs. Thread:**
    *   **Process:** An isolated *container* (like a house). It has a Process ID (PID), a current working directory, and memory permissions.
    *   **Thread:** An active *worker* living inside that house. A process can have multiple threads. Every thread has its own instruction pointer (`RIP`) and its own private stack (`RSP`).
*   **Context Switch:**
    The process of freezing Thread A (saving all its CPU registers to its stack), switching the CPU's stack pointer to Thread B's stack, and unfrozen Thread B (restoring its registers).
*   **Callee-Saved Registers (System V ABI):**
    The AMD64 calling convention rules say that functions are allowed to scramble `%rax`, `%rcx`, `%rdx`, `%rsi`, `%rdi`, `%r8..r11`. But if a function touches `%rbx`, `%rbp`, `%r12`, `%r13`, `%r14`, or `%r15`, it **must** save them and restore them before returning.
    Therefore, to switch tasks cooperatively, we only need to save the callee-saved registers and `RFLAGS`!
*   **Cooperative vs. Preemptive Multitasking:**
    *   **Cooperative:** Tasks voluntarily give up the CPU by calling `yield_now()`.
    *   **Preemptive:** The hardware timer interrupts a running task without asking and forcibly swaps it out.
*   **Zombie State:**
    When a process finishes, it cannot immediately vanish. Its exit code must be kept in memory until its parent process calls `wait_pid()` to read the result. A finished process waiting to be reaped is called a **Zombie**.

---

## 🗺️ What Files are Involved?
1. [src/task/process.rs](file:///home/aj/BeanieOS/src/task/process.rs) — Manages processes, PIDs, parents, and statuses.
2. [src/task/scheduler.rs](file:///home/aj/BeanieOS/src/task/scheduler.rs) — Thread queues, stack preparation, and the naked assembly `switch_context` routine.
3. [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L161-L162) — Initializes `process::init()` and `scheduler::init()`.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Crafting a Brand New Thread's Stack
Located at lines 66–95 of [src/task/scheduler.rs](file:///home/aj/BeanieOS/src/task/scheduler.rs#L66-L95):

When we spawn a new thread, how do we make the CPU run its entry function for the first time?
**We forge a fake stack that looks like the thread was already running and just got switched out!**

```rust
pub fn new(pid: ProcessId, name: &str, entry: extern "C" fn(), priority: Priority) -> Self {
    // 1. Allocate a fresh 64 KiB stack on the heap
    let stack = vec![0u8; STACK_SIZE].into_boxed_slice();
    let stack_top = stack.as_ptr() as usize + STACK_SIZE;
    let mut sp = stack_top & !0xf; // 16-byte alignment

    // 2. Pre-plant return addresses
    sp -= 8;
    unsafe { *(sp as *mut usize) = thread_trampoline_exit as usize };
    sp -= 8;
    unsafe { *(sp as *mut usize) = entry as usize }; // Starting function

    // 3. Pre-plant 7 registers: rbp, rbx, r12, r13, r14, r15, rflags
    sp -= 7 * 8;
    unsafe {
        core::ptr::write_bytes(sp as *mut u8, 0, 7 * 8);
        // Bit 9 of RFLAGS = 0x200 (Interrupt Flag enabled!)
        let rflags_ptr = sp.wrapping_add(6 * 8) as *mut usize;
        *rflags_ptr = 0x200;
    }

    Thread { id, pid, name, state: ThreadState::Ready, priority, rsp: sp, stack: Some(stack) }
}
```

---

### Step 2: The Magic Assembly Context Switch
Located at lines 241–265 of [src/task/scheduler.rs](file:///home/aj/BeanieOS/src/task/scheduler.rs#L241-L265):

This naked assembly function is where the miracle happens:
```assembly
.global switch_context
switch_context:
    # rdi contains pointer to old_thread.rsp
    # rsi contains new_thread.rsp

    # 1. Save all callee-saved registers onto the CURRENT thread's stack
    pushfq
    push %r15
    push %r14
    push %r13
    push %r12
    push %rbx
    push %rbp

    # 2. Save the current stack pointer into old_thread.rsp
    mov %rsp, (%rdi)

    # 3. THE SWITCH: Load the NEW thread's stack pointer!
    mov %rsi, %rsp

    # 4. We are now running on the NEW thread's stack!
    # Pop all saved registers off the NEW thread's stack
    pop %rbp
    pop %rbx
    pop %r12
    pop %r13
    pop %r14
    pop %r15
    popfq

    # 5. Return! Pops the address off the new stack into RIP.
    # The CPU instantly resumes executing the new thread!
    ret
```
Notice how simple and brilliant this is: **By simply changing the `%rsp` register, the entire execution context changes.**

---

### Step 3: Priority Scheduling (`pick_next`)
Located at lines 149–210 of [src/task/scheduler.rs](file:///home/aj/BeanieOS/src/task/scheduler.rs#L149-L210):

The scheduler runs a 4-level priority system:
```
Priority 0 (High)   -> [ Thread A ] -> [ Thread B ]
Priority 1 (Normal) -> [ Thread C ]
Priority 2 (Low)    -> [ Thread D ]
Priority 3 (Idle)   -> [ Idle Thread ]
```
1.  **Sleep Wakeups:** Checks any sleeping threads. If `current_ticks >= wake_tick`, marks them `Ready` and moves them back to their ready queue.
2.  **Highest Queue Search:** Scans queues from High (0) to Idle (3). Pops the first ready `ThreadId`.
3.  **Prepare Stack Pointers:** Retrieves `prev_rsp_ptr` from the current thread and `next_rsp` from the chosen thread.
4.  **Drop Locks & Switch:** Calls `switch_context(prev_rsp_ptr, next_rsp)` after releasing mutex locks to prevent deadlocks.

---

## 🎯 Summary Checklist
By the end of Phase 9, our operating system has:
1. Implemented a `ProcessManager` tracking processes, PIDs, and parent-child states.
2. Created a `Thread` abstraction with dedicated 64 KiB stacks.
3. Implemented a hand-crafted assembly `switch_context` function capable of swapping running tasks in microseconds.
4. Built a priority-based round-robin scheduler with sleep/wake support.
