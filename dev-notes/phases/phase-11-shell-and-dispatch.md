# Phase 11: Interactive Shell, Keyboard Pipeline, & Kernel Dispatch Loop

## 🌟 High-Level Overview
We have reached the culmination of our operating system's startup sequence! 

Up to this point, our kernel has built all the invisible machinery: 64-bit paging, memory allocators, graphics drivers, interrupt tables, storage drivers, and the scheduler.

Now, we bring the system to life:
1.  We spawn an interactive **Shell Process** to handle user commands.
2.  We connect the hardware **Keyboard Pipeline** (from physical port `0x60`, through the interrupt handler, into a thread-safe keystroke queue, and into the shell).
3.  We spawn an auxiliary background thread (`rgb_square_task`) that animates a pulsing, color-cycling square on the screen to visually prove that multitasking is working.
4.  The main kernel thread enters the **Idle Dispatch Loop**, executing `hlt` instructions to save electricity whenever there is nothing to run.

---

## 📖 Layman's Glossary: Jargon Demystified

*   **Scancode:**
    When you press a key on a physical keyboard, it does *not* send the letter 'A'. It sends an arbitrary hardware number called a **Scancode** (e.g., pressing key 'Q' sends `0x10`; releasing 'Q' sends `0x90`).
*   **PS/2 Data Port (`0x60`):**
    The legacy I/O port where the motherboard's keyboard controller places the scancode byte for the CPU to read.
*   **Interrupt-Driven Input:**
    Instead of making the shell freeze the entire computer in a `while true` loop constantly checking "did you type yet?", the shell sleeps. The hardware interrupt wakes the system up only when an actual physical keystroke happens.
*   **Producer-Consumer Queue:**
    The interrupt handler *produces* keystrokes and pushes them into a lock-free queue (`push_key`). The shell *consumes* keystrokes and pops them out (`pop_key`). This decoupling ensures that disk reads or slow terminal printing never happen inside an interrupt context!
*   **`HLT` (Halt Instruction):**
    Puts the CPU core into a low-power slumber state until the next hardware interrupt arrives. Without `hlt`, a CPU core stuck in a `loop {}` will spin at 100% capacity, burning electricity and heating up the room!

---

## 🗺️ What Files are Involved?
1. [src/arch/interrupts/vectors.rs](file:///home/aj/BeanieOS/src/arch/interrupts/vectors.rs) — The keyboard interrupt handler, PS/2 port reading, scancode mapping, and `TICKS`.
2. [src/shell.rs](file:///home/aj/BeanieOS/src/shell.rs) — The keystroke queue, command parser (`ls`, `cat`, etc.), and shell prompt.
3. [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L35-L95) — Spawning the shell task, the RGB animation task, and the idle dispatch loop.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: The Keystroke Journey (Hardware to Interrupt)
Located at lines 18–33 of [src/arch/interrupts/vectors.rs](file:///home/aj/BeanieOS/src/arch/interrupts/vectors.rs#L18-L33):

When you press a key:
1.  The keyboard hardware fires GSI 1, which the I/O APIC routes to Vector 33.
2.  The CPU pauses whatever it was doing and calls `keyboard_interrupt_handler`:
    ```rust
    extern "x86-interrupt" fn keyboard_interrupt_handler(
        _stack_frame: InterruptStackFrame,
    ) {
        // 1. Read the scancode from I/O Port 0x60
        let mut port = Port::new(PS2_DATA_PORT);
        let scancode: u8 = unsafe { port.read() };

        // 2. Translate Scancode to ASCII ('A', 'B', '\n', etc.)
        if let Some(inp) = scancode_to_ascii(scancode) {
            crate::shell::push_key(inp); // Push to consumer queue
        }

        // 3. Acknowledge the interrupt so LAPIC can send more!
        unsafe { pic::eoi(); }
    }
    ```

---

### Step 2: The Shell Task Worker
Located at lines 63–72 of [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L63-L72):

The shell runs as an independent thread. It drains the queue:
```rust
extern "C" fn shell_task() {
    let shell = SHELL.get().expect("Shell not initialized");
    loop {
        if let Some(key) = shell::pop_key() {
            shell.lock().shell_input(key); // Process character, run commands
        } else {
            task::scheduler::yield_now(); // No keys typed? Give up the CPU!
        }
    }
}
```
*   Because the shell runs in thread context (outside the interrupt handler), it is completely safe for shell commands to read the SATA disk, allocate heap memory, or print large text files without blocking CPU interrupts!

---

### Step 3: Demonstrating Multitasking (`rgb_square_task`)
Located at lines 76–95 of [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L76-L95):

To visually prove that multiple threads are running concurrently without interfering with each other, we spawn a second worker task:
```rust
extern "C" fn rgb_square_task() {
    loop {
        // Calculate a shifting color rainbow based on tick count
        let tick = RGB_TICKS.fetch_add(8, Ordering::Relaxed) % 1536;
        let (r, g, b) = match tick {
            0..=255 => (255, tick, 0),
            256..=511 => (511 - tick, 255, 0),
            // ...
        };
        let color = (r << 16) | (g << 8) | b;

        // Draw a 100x100 pixel square directly onto the screen
        WRITER.lock().fill_rect(500, 500, 100, 100, color);

        // Voluntarily yield so the shell and other threads can run!
        task::scheduler::yield_now();
    }
}
```

---

### Step 4: Spawning the Processes and Entering the Idle Loop
Located at lines 40–61 of [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L40-L61):

In `kernel_main`:
```rust
    // 1. Create Process & Thread for the Shell
    let shell_pid = task::process::create_process("shell", "/");
    task::scheduler::spawn_in_process(
        shell_pid, "shell", shell_task, task::scheduler::Priority::Normal,
    );

    // 2. Create Process & Thread for the RGB Animation
    let rgb_pid = task::process::create_process("rgb_square", "/");
    task::scheduler::spawn_in_process(
        rgb_pid, "rgb_square", rgb_square_task, task::scheduler::Priority::Normal,
    );

    // 3. The Kernel Master Idle Loop
    loop {
        task::scheduler::yield_now(); // Run any ready tasks
        x86_64::instructions::hlt();  // Sleep CPU until next hardware interrupt!
    }
```

---

## 🎯 Summary Checklist
By the end of Phase 11:
1. The hardware keyboard interrupt handler intercepts every keypress on port `0x60`.
2. Scancodes are safely translated to ASCII and queued in memory.
3. The interactive `shell_task` processes commands and mounts the FAT disk.
4. The `rgb_square_task` renders real-time graphics concurrently.
5. The kernel enters a power-efficient idle loop governed by `hlt` and `yield_now()`.
6. **BeanieOS is fully initialized, alive, and ready for user commands!**
