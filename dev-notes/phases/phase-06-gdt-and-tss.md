# Phase 6: Global Descriptor Table (GDT) & Task State Segment (TSS)

## 🌟 High-Level Overview
In Phase 1, our assembly code loaded a tiny, barebones GDT just to jump into 64-bit mode. But that early table was only a temporary scaffold.

Now, we must set up the **permanent, architecture-compliant Global Descriptor Table (GDT)** and its critical partner, the **Task State Segment (TSS)**.

Why is this so urgent? Because of a terrifying hardware scenario known as a **Triple Fault**. 
If your kernel code ever runs out of stack memory (a stack overflow), the CPU tries to trigger a Page Fault exception. But to trigger an exception, the CPU must push error details onto... the stack! Since the stack is broken, that push fails, triggering a Double Fault exception. But pushing the Double Fault onto the broken stack *also* fails! At that point, the CPU panics, gives up, and hard-resets the physical computer.

The TSS and its **Interrupt Stack Table (IST)** are our superhero cape: they give the CPU a guaranteed fresh, uncorrupted backup stack so it can safely catch Double Faults and print diagnostics instead of instantly rebooting.

---

## 📖 Layman's Glossary: Jargon Demystified

*   **Segmentation:**
    An ancient memory management scheme from the 1980s where memory was divided into overlapping "segments" (Code Segment `CS`, Data Segment `DS`, Stack Segment `SS`). In 64-bit mode, segmentation is turned off—all segments have their base set to 0 and span the whole address space.
*   **GDT (Global Descriptor Table):**
    Even though segmentation is mostly dead, the 64-bit CPU still requires a GDT. Why?
    1.  The Code Segment (`CS`) tells the CPU: *"Are we in 64-bit mode or 32-bit mode? Are we Ring 0 (Kernel) or Ring 3 (User)?"*
    2.  The GDT holds the pointer to the TSS.
*   **TSS (Task State Segment):**
    In old 32-bit chips, the TSS was used for automatic hardware multitasking (which was slow and nobody liked). In 64-bit mode, AMD redesigned the TSS. Now, it has two jobs:
    1.  Hold the kernel stack pointer (`RSP0`) for when user programs call into the kernel.
    2.  Hold the **Interrupt Stack Table (IST)**.
*   **IST (Interrupt Stack Table):**
    A list of up to 7 dedicated, pre-allocated emergency backup stack pointers inside the TSS. You can configure individual exceptions (like Double Fault) to switch to an IST stack automatically.
*   **Triple Fault:**
    Fault 1 (e.g. Stack Overflow) $\rightarrow$ Fault 2 (Double Fault fails to push to stack) $\rightarrow$ **Triple Fault** (CPU shuts down immediately).
*   **Segment Selectors:**
    An offset index into the GDT (e.g., `0x08` for code, `0x10` for data).

---

## 🗺️ What Files are Involved?
1. [src/arch/gdt.rs](file:///home/aj/BeanieOS/src/arch/gdt.rs) — Allocates the emergency stack, builds the TSS and GDT, and reloads segment registers.
2. [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L157) — Calls `arch::gdt::init()`.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Allocating the Emergency Double Fault Stack
Located at lines 6–21 of [src/arch/gdt.rs](file:///home/aj/BeanieOS/src/arch/gdt.rs#L6-L21):

```rust
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        // Configure IST slot 0 with a dedicated 20 KiB emergency stack
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5; // 20 KiB
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            // Point to the TOP of the stack (grows downwards)
            let stack_start = VirtAddr::from_ptr(&raw const STACK);
            stack_start + STACK_SIZE as u64
        };
        tss
    };
}
```
*   `lazy_static!` ensures this TSS structure is safely initialized at runtime and lives in memory forever (`'static`).
*   Whenever an exception uses `DOUBLE_FAULT_IST_INDEX`, the CPU's hardware will forcibly overwrite `%rsp` with this 20 KiB emergency stack top before executing a single instruction of the handler.

---

### Step 2: Assembling the 64-Bit GDT
Located at lines 23–36 of [src/arch/gdt.rs](file:///home/aj/BeanieOS/src/arch/gdt.rs#L23-L36):

```rust
lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        
        // 1. Kernel Code Segment (Executable, Ring 0, 64-bit Long Mode bit set)
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        
        // 2. Task State Segment Descriptor (Points to our static TSS above)
        let tss_selector = gdt.append(Descriptor::tss_segment(&TSS));
        
        (gdt, Selectors { code_selector, tss_selector })
    };
}
```
*   In 64-bit mode, a TSS descriptor is unique: it takes up **16 bytes** (two GDT slots) instead of the normal 8 bytes, because it needs to hold a full 64-bit memory address pointing to the TSS struct!

---

### Step 3: Loading the GDT and Task Register
Located at lines 43–57 of [src/arch/gdt.rs](file:///home/aj/BeanieOS/src/arch/gdt.rs#L43-L57):

```rust
pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    // 1. Load the new table into the CPU's GDTR register (lgdt instruction)
    GDT.0.load();

    unsafe {
        // 2. Reload the Code Segment register (CS)
        CS::set_reg(GDT.1.code_selector);
        
        // 3. Clear data segment registers to null
        DS::set_reg(SegmentSelector(0));
        ES::set_reg(SegmentSelector(0));
        FS::set_reg(SegmentSelector(0));
        GS::set_reg(SegmentSelector(0));
        SS::set_reg(SegmentSelector(0));

        // 4. Load the Task Register (ltr instruction)
        load_tss(GDT.1.tss_selector);
    }
}
```
*   `CS::set_reg`: You cannot use a regular `mov` instruction to change `CS`. In the background, this performs a far return / far jump to reload `CS`.
*   `load_tss`: Executes the x86 `ltr` (Load Task Register) instruction. The CPU reads the TSS descriptor from the GDT, caches its address, and marks the TSS as "busy".

---

## 🎯 Summary Checklist
By the end of Phase 6, our operating system has:
1. Created an isolated 20 KiB emergency stack for Double Faults.
2. Constructed a Task State Segment (TSS) with an Interrupt Stack Table (IST).
3. Created a 64-bit GDT containing the kernel code segment and 16-byte TSS descriptor.
4. Loaded the GDT (`lgdt`), updated `CS`, and loaded the Task Register (`ltr`).
5. Made the kernel immune to catastrophic triple-fault boot loops!
