# Phase 7: Interrupt Descriptor Table (IDT) & CPU Exception Handlers

## 🌟 High-Level Overview
Computers do not execute code in a peaceful, predictable bubble. Unexpected events happen all the time:
1.  **Software Errors (Exceptions):** A programmer tries to divide by zero, executes a corrupted instruction, or tries to read unmapped virtual memory.
2.  **Hardware Signals (Interrupts):** A user taps a key on the keyboard, or a hardware timer rings to signal that a millisecond has passed.

When one of these events happens, the CPU immediately pauses whatever it was doing, saves its place, and looks up what to do in a master dispatch phonebook called the **Interrupt Descriptor Table (IDT)**.

In this phase, we:
1. Construct the 256-entry 64-bit IDT.
2. Write handlers for all major CPU exceptions (Divide Error, Page Fault, General Protection Fault, etc.).
3. Connect the Double Fault handler to the emergency IST stack we created in Phase 6.
4. Load the IDT into the CPU's `IDTR` register using the `lidt` instruction.

---

## 📖 Layman's Glossary: Jargon Demystified

*   **Interrupt vs. Exception:**
    *   **CPU Exception (Synchronous):** The CPU generates this *itself* because the current instruction did something impossible or forbidden (e.g., dividing by zero or accessing memory that doesn't exist).
    *   **Hardware Interrupt (Asynchronous):** A device outside the CPU (like a keyboard, timer, or network card) pulls an electrical wire to say: *"Hey! I have new data for you!"*
*   **Vector Number (0 to 255):**
    Each event has a unique number:
    *   **Vectors 0–31:** Reserved by Intel/AMD strictly for CPU exceptions.
        *   `0`: Divide Error (divide by zero)
        *   `6`: Invalid Opcode (CPU encountered unreadable instructions)
        *   `8`: Double Fault
        *   `13`: General Protection Fault (GPF / `#GP`, usually permissions or bad segment)
        *   `14`: Page Fault (`#PF`, tried to access an unmapped virtual address)
    *   **Vectors 32–255:** Free for the operating system to assign to hardware devices and system calls.
*   **The Hardware Interrupt Stack Frame:**
    When an interrupt fires, the CPU hardware *automatically* pushes 5 critical values onto the stack before running our code:
    $$\text{Stack} \leftarrow [\text{SS}, \text{RSP}, \text{RFLAGS}, \text{CS}, \text{RIP}]$$
    Some exceptions also push an **Error Code** number on top!
*   **`iretq` (Interrupt Return 64-bit):**
    A special CPU instruction used to return from an interrupt. It pops all 5 values back into their respective CPU registers, restoring the program *exactly* as it was before the interrupt happened.
*   **`extern "x86-interrupt"`:**
    A specialized calling convention feature in the Rust compiler. It tells Rust: *"This is an interrupt handler! Do not treat it like a normal function. Save all CPU registers so you don't overwrite the caller's work, and finish with `iretq` instead of `ret`."*
*   **`CR2` Register:**
    When a Page Fault occurs, the CPU automatically stores the exact virtual address that caused the crash in Control Register `%cr2`.

---

## 🗺️ What Files are Involved?
1. [src/arch/interrupts/mod.rs](file:///home/aj/BeanieOS/src/arch/interrupts/mod.rs) — Instantiates and loads the `IDT`.
2. [src/arch/interrupts/faults.rs](file:///home/aj/BeanieOS/src/arch/interrupts/faults.rs) — Handlers for all 20+ CPU fault exceptions.
3. [src/arch/interrupts/vectors.rs](file:///home/aj/BeanieOS/src/arch/interrupts/vectors.rs) — Handlers for hardware timer, keyboard, and spurious interrupts.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: The Exception Handlers Macro
Located at lines 9–36 of [src/arch/interrupts/faults.rs](file:///home/aj/BeanieOS/src/arch/interrupts/faults.rs#L9-L36):

Writing 20 separate identical functions is tedious, so we use a Rust macro to generate them:
```rust
macro_rules! fault_handler {
    ($name:ident, $label:literal) => {
        extern "x86-interrupt" fn $name(stack_frame: InterruptStackFrame) {
            crate::println!(concat!("EXCEPTION: ", $label, "\n{:#?}"), stack_frame);
            loop {
                x86_64::instructions::hlt();
            }
        }
    };
}
```
*   When a fault occurs, it prints the exception name along with the complete stack frame (Instruction Pointer `RIP`, Stack Pointer `RSP`, and `RFLAGS`) so the developer can see the exact line of code that crashed.

---

### Step 2: Specialized Handlers (Page Fault & Double Fault)
Located at lines 70–90 of [src/arch/interrupts/faults.rs](file:///home/aj/BeanieOS/src/arch/interrupts/faults.rs#L70-L90):

```rust
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    crate::println!("EXCEPTION: PAGE FAULT");
    crate::println!("Accessed Address: {:?}", Cr2::read());
    crate::println!("Error Code: {:?}", error_code);
    crate::println!("{:#?}", stack_frame);
    loop {
        x86_64::instructions::hlt();
    }
}
```
*   In `page_fault_handler`, `Cr2::read()` reveals the guilty address. The `PageFaultErrorCode` reveals *why* it faulted: Was it a read or write? Was the page not present, or was it a privilege violation?

---

### Step 3: Wiring Up the Table
Located at lines 92–121 of [src/arch/interrupts/faults.rs](file:///home/aj/BeanieOS/src/arch/interrupts/faults.rs#L92-L121):

```rust
pub(crate) fn register_faults(idt: &mut InterruptDescriptorTable) {
    idt.divide_error.set_handler_fn(divide_error_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.general_protection_fault.set_handler_fn(general_protection_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);

    // CRITICAL: Connect Double Fault to our Phase 6 emergency stack!
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    // ... registers all remaining exceptions ...
}
```

---

### Step 4: Registering Hardware Vectors
Located at lines 48–52 of [src/arch/interrupts/vectors.rs](file:///home/aj/BeanieOS/src/arch/interrupts/vectors.rs#L48-L52):

We map our hardware device interrupts to custom vector numbers:
```rust
pub(crate) fn register_vectors(idt: &mut InterruptDescriptorTable) {
    idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_interrupt_handler); // Vector 33
    idt[LAPIC_TIMER_VECTOR].set_handler_fn(timer_interrupt_handler);  // Vector 32
    idt[SPURIOUS_VECTOR].set_handler_fn(spurious_interrupt_handler);    // Vector 255
}
```

---

### Step 5: Loading the IDT into Hardware
Located at lines 18–20 of [src/arch/interrupts/mod.rs](file:///home/aj/BeanieOS/src/arch/interrupts/mod.rs#L18-L20):

```rust
pub fn init_idt(acpi_root_addr: usize) {
    IDT.load(); // Executes the `lidt` assembly instruction
    // ...
}
```
The CPU's `IDTR` register is updated with the memory location and size of our table. From this exact millisecond forward, if any CPU exception or registered interrupt occurs, the CPU will jump directly to our Rust functions!

---

## 🎯 Summary Checklist
By the end of Phase 7, our operating system has:
1. Created an `InterruptDescriptorTable` with 256 gates.
2. Registered informative diagnostic handlers for every CPU exception vector (0–21).
3. Bound the Double Fault handler to the Phase 6 emergency stack.
4. Registered hardware vectors for the Timer (32) and Keyboard (33).
5. Loaded the table into the CPU via the `lidt` instruction.
