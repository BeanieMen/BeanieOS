# Phase 1: Boot Assembly & Entering 64-Bit Long Mode

## 🌟 High-Level Overview
When an x86 computer turns on, it does **not** wake up as a modern 64-bit powerhouse. Because of 45+ years of backward compatibility dating back to the 1978 Intel 8086 chip, the processor starts up acting like an ancient, primitive computer.

In this phase, our kernel takes its very first breath. We take the CPU from **32-bit Protected Mode** (handed to us by the bootloader) and manually unlock **64-bit Long Mode**. Without this phase, 64-bit Rust code cannot run, 64-bit registers do not exist, and memory beyond 4 GiB cannot be accessed.

---

## 📖 Layman's Glossary: Jargon Demystified
Before looking at code, here is what all the obscure hardware words actually mean:

*   **Processor Register (`%eax`, `%esp`, `%cr0`, `%rdi`, etc.):**
    Think of a register as a tiny, ultra-fast scratchpad right inside the CPU core. RAM is like a warehouse full of notebooks down the street; registers are the sticky notes directly in the CPU's hand.
    *   32-bit registers start with `E` (e.g., `EAX`, `EBX`, `ESP`, `EBP`).
    *   64-bit registers start with `R` (e.g., `RAX`, `RBX`, `RSP`, `RBP`, `RDI`, `RSI`).
*   **CPU Modes:**
    *   **Real Mode (16-bit):** The raw 1978 mode. Can only address 1 megabyte of RAM. No security, no memory protection.
    *   **Protected Mode (32-bit):** Introduced in 1985 (Intel 386). Allows addressing up to 4 gigabytes of RAM. Supports basic memory security.
    *   **Long Mode (64-bit):** Introduced by AMD in 2003 (AMD64 / x86_64). Enables 64-bit math, 16 general-purpose 64-bit registers, and theoretical exabytes of memory.
*   **Paging & Virtual Memory:**
    A trick where the CPU pretends to programs that memory is laid out in a nice, neat, continuous line (Virtual Address), while secretly slicing it into 4,096-byte blocks (called **Pages**) scattered all over physical RAM chips (called **Frames**).
*   **Page Tables (P4, P3, P2, P1):**
    A 4-tiered lookup tree (like an address book: Country -> State -> City -> Street) that the CPU's hardware walks through to translate a virtual address into a physical RAM address.
*   **Control Registers (`CR0`, `%cr3`, `%cr4`):**
    Special master switches on the CPU chip. Flipping specific bits in these registers turns major CPU features on or off (like turning on Paging, PAE, or Protected Mode).
*   **MSR (Model-Specific Register):**
    Special internal CPU settings registers accessed with special instructions (`rdmsr` / `wrmsr`).
*   **EFER (Extended Feature Enable Register):**
    A specific MSR (`0xC000_0080`) that contains the master switch bit for 64-bit Long Mode (`LME`).
*   **Stack & Stack Pointer (`%esp` / `%rsp`):**
    A region of memory used like a stack of cafeteria plates. When functions are called or variables are saved, data is "pushed" onto the top. `%esp` (32-bit) or `%rsp` (64-bit) holds the memory address of the top plate.
*   **GDT (Global Descriptor Table):**
    A legacy table describing memory "segments". In 64-bit mode, it is mostly a formality, but the CPU still demands it to know: "Are we executing 64-bit code, 32-bit code, or user-space code?"

---

## 🗺️ What Files are Involved?
1. [src/arch/boot.s](file:///home/aj/BeanieOS/src/arch/boot.s) — Raw assembly language instructions executed the instant the bootloader jumps to us.
2. [src/arch/boot.rs](file:///home/aj/BeanieOS/src/arch/boot.rs) — Defines the pre-calculated page table data structures in memory.
3. [linker.ld](file:///home/aj/BeanieOS/linker.ld) — Instructs the compiler/linker to place `_start` right at the very beginning of the executable.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Entry Point & Clearing Flags
Located at lines 9–11 of [src/arch/boot.s](file:///home/aj/BeanieOS/src/arch/boot.s#L9-L11):
```assembly
.global _start
_start:
    cli
    cld
```
*   **Why?**
    *   `cli` stands for **Clear Interrupts**. When turning on an OS, hardware devices (like keyboards, timers, or mouse) might send electrical signals ("interrupts"). Because our Interrupt Table (IDT) is not set up yet, receiving an interrupt right now would crash the machine instantly.
    *   `cld` stands for **Clear Direction Flag**. It ensures string and memory copy instructions (like `stosl` or `movsb`) process forward from low addresses to high addresses.

---

### Step 2: Saving Bootloader Information & Setting Up a Temporary Stack
Located at lines 13–16 of [src/arch/boot.s](file:///home/aj/BeanieOS/src/arch/boot.s#L13-L16):
```assembly
    mov %eax, %ebp              # multiboot magic -> callee-saved for later
    mov %ebx, %esi              # multiboot info addr -> later 2nd arg

    mov $BOOT_STACK+65536, %esp
```
*   **What is happening?**
    The bootloader left two crucial pieces of information in the CPU:
    1.  `%eax` contains a "secret handshake" magic number (`0x36d76289`) proving a compliant Multiboot2 bootloader ran.
    2.  `%ebx` contains the physical RAM address of a data packet describing the computer's memory, screen, and hardware.
    We move these values into `%ebp` and `%esi` so our upcoming calculations don't overwrite them.
*   **The Stack:**
    In assembly, you cannot call functions (`call`) without a stack, because the CPU needs somewhere to write down the return address. `BOOT_STACK` is an uninitialized 64 KiB buffer defined in Rust ([src/arch/boot.rs](file:///home/aj/BeanieOS/src/arch/boot.rs#L125)). Stacks on x86 grow **downwards**, so we set the stack pointer `%esp` to the *top* of the buffer (`BOOT_STACK + 65536`).

---

### Step 3: Zeroing Out Page Tables (P4 and P3)
Located at lines 21–28 of [src/arch/boot.s](file:///home/aj/BeanieOS/src/arch/boot.s#L21-L28):
```assembly
    mov $P4_TABLE, %edi
    xor %eax, %eax
    mov $1024, %ecx
    rep stosl                   # zero P4 (4 KiB)

    mov $P3_TABLE, %edi
    mov $1024, %ecx
    rep stosl                   # zero P3 (4 KiB)
```
*   **Why?**
    In 64-bit mode, the CPU *refuses* to operate without Paging enabled. If a page table contains random leftover garbage from RAM power-on, the CPU will think invalid pages exist and crash.
    *   `rep stosl` repeats `1024` times, writing 4 zero-bytes each time (`1024 * 4 = 4096` bytes = 4 KiB, the exact size of one hardware page table).

---

### Step 4: Connecting the Page Table Hierarchy
Located at lines 30–48 of [src/arch/boot.s](file:///home/aj/BeanieOS/src/arch/boot.s#L30-L48):
```assembly
    mov $P3_TABLE, %eax
    or $0x3, %eax               # present (bit 0) | writable (bit 1)
    mov %eax, P4_TABLE
    movl $0, P4_TABLE + 4

    mov $0, %ecx

.map_p3_table:
    mov $4096, %eax
    mul %ecx
    add $P2_TABLES, %eax
    or $0x3, %eax               # present | writable

    mov %eax, P3_TABLE(, %ecx, 8)
    movl $0, P3_TABLE + 4(, %ecx, 8)

    inc %ecx
    cmp $8, %ecx
    jne .map_p3_table
```
*   **What is this tree structure?**
    64-bit x86 uses a 4-level hierarchy:
    ```
    CR3 Register -> Level 4 Table (PML4 / P4)
                       |
                       +--> Level 3 Table (PDPT / P3)
                              |
                              +--> Level 2 Tables (PD / P2) [8 tables]
                                     |
                                     +--> 2 MiB Huge Pages (Direct Physical Memory)
    ```
*   **The Math:**
    Each entry in a Level 2 table can map a **2 Megabyte Huge Page** (by setting the "huge page" bit `0x80`).
    *   1 Level 2 table has 512 entries = `512 * 2 MiB = 1 GiB`.
    *   We map 8 Level 2 tables into `P3_TABLE` = `8 * 1 GiB = 8 GiB` of physical memory!
    *   The entries in `P2_TABLES` are already pre-calculated at compile time in Rust ([src/arch/boot.rs](file:///home/aj/BeanieOS/src/arch/boot.rs#L100-L112)) to point directly to memory addresses `0x0`, `0x200000`, `0x400000`, etc.
    *   `or $0x3, %eax` sets bit 0 (Present in memory) and bit 1 (Writable).

---

### Step 5: The Hardware Long-Mode Incantation
Located at lines 50–64 of [src/arch/boot.s](file:///home/aj/BeanieOS/src/arch/boot.s#L50-L64):
To awaken 64-bit mode, Intel and AMD dictate an exact hardware sequence:
```assembly
    # 1. Tell the CPU where the root page table is
    mov $P4_TABLE, %eax
    mov %eax, %cr3

    # 2. Enable PAE (Physical Address Extension)
    mov %cr4, %eax
    or $(1 << 5), %eax          # Bit 5 = PAE
    mov %eax, %cr4

    # 3. Enable Long Mode in the EFER Model-Specific Register
    mov $0xc0000080, %ecx       # Address of EFER MSR
    rdmsr                       # Read MSR into EDX:EAX
    or $(1 << 8), %eax          # Bit 8 = LME (Long Mode Enable)
    wrmsr                       # Write back to MSR

    # 4. Turn on Paging and Protected Mode
    mov %cr0, %eax
    or $(1 << 31 | 1), %eax     # Bit 31 = PG (Paging), Bit 0 = PE (Protection)
    mov %eax, %cr0
```
At this exact moment after `mov %eax, %cr0`, the CPU is in **Compatibility Mode** (paging is 64-bit, but code execution is still 32-bit).

---

### Step 6: The Long Jump into 64-Bit Mode
Located at lines 66–68 of [src/arch/boot.s](file:///home/aj/BeanieOS/src/arch/boot.s#L66-L68):
```assembly
    lgdt gdt64_pointer
    ljmp $0x08, $long_mode_start
```
*   `lgdt`: Loads our temporary 64-bit Global Descriptor Table. In this table ([lines 100–109](file:///home/aj/BeanieOS/src/arch/boot.s#L100-L109)), entry `0x08` has the `L` bit (bit 53) set, designating it as a **64-bit Code Segment**.
*   `ljmp $0x08, $long_mode_start`: A regular `jmp` only changes the instruction pointer (`EIP`). A **Far Jump** (`ljmp`) forces the CPU to reload the Code Segment register (`CS`) with `0x08`. This flushes the CPU's internal 32-bit pipeline and officially engages 64-bit sub-mode!

---

### Step 7: Landing in 64-Bit World (`.code64`)
Located at lines 73–86 of [src/arch/boot.s](file:///home/aj/BeanieOS/src/arch/boot.s#L73-L86):
```assembly
.code64
long_mode_start:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    mov $BOOT_STACK+65536, %rsp

    mov %ebp, %edi              # magic -> 1st argument in System V ABI
    mov %esi, %esi              # mbi addr -> 2nd argument (clears top 32 bits)

    call rust_entry
```
*   In 64-bit mode, data segments are mostly ignored, but setting them to `0x10` (our 64-bit Data Segment) prevents CPU warnings.
*   We upgrade `%esp` to `%rsp` (the full 64-bit stack pointer).
*   **System V AMD64 Calling Convention:**
    When calling a C or Rust function in 64-bit mode on Linux/BSD, arguments are passed in registers:
    *   Argument 1 goes into `%rdi`
    *   Argument 2 goes into `%rsi`
    We move our saved Multiboot magic into `%rdi` and the MBI physical address into `%rsi`.
*   Finally: `call rust_entry`. We are now leaving assembly and entering Rust!

---

## 🎯 Summary Checklist
By the end of Phase 1, our operating system has:
1. Silenced hardware interrupts so early initialization isn't disrupted.
2. Identity-mapped 8 gigabytes of RAM using 2 MiB huge pages.
3. Enabled PAE, Long Mode, and Paging.
4. Loaded a 64-bit GDT and performed a far jump.
5. Successfully prepared 64-bit registers and entered Rust!
