# Phase 2: Rust Entry Point & Multiboot2 Protocol Validation

## 🌟 High-Level Overview
In Phase 1, our assembly code successfully dragged the CPU into 64-bit mode and jumped to `rust_entry`. But here is the problem: **Rust normally assumes an operating system already exists!** 

When you write a normal Rust or C program, an operating system like Linux or Windows is already running in the background. The OS sets up memory, opens files, and provides standard libraries (`std`). In kernel development, **we are the operating system**. There is no standard library (`#![no_std]`), no runtime, and no safety net.

In this phase, we make our grand entrance into Rust. We verify that the bootloader kept its promises, inspect the bootloader's metadata packet (the **Multiboot2 Information Structure**), and jump to `kernel_main`.

---

## 📖 Layman's Glossary: Jargon Demystified

*   **Bootloader (GRUB, Limine, etc.):**
    A small program stored in your computer's flash drive, hard disk, or UEFI firmware. Its only job is to find the kernel file on the disk, copy it into RAM, and jump to it.
*   **The Multiboot2 Specification:**
    In the early days of PC development, every single OS had to invent its own custom bootloader. If you wanted to run Linux, you needed a Linux bootloader; for FreeBSD, a FreeBSD bootloader.
    To solve this chaos, the **Multiboot2 standard** was created. It is a universal contract:
    *   *The Kernel says:* "Here is a tiny stamp (the Multiboot Header) embedded in my binary saying what hardware I want."
    *   *The Bootloader says:* "I will load you, set up the screen and memory according to your stamp, and hand you a receipt (the Multiboot Information structure) detailing what I did."
*   **Magic Number:**
    A unique, hard-coded 32-bit number used as a digital secret handshake. If the bootloader leaves the exact magic number in register `%eax`, our kernel knows the bootloader is genuine and didn't crash or garble memory.
*   **`#![no_std]`:**
    A directive telling the Rust compiler: *"Do not include the standard library! We don't have filesystems, threads, network sockets, or `println!` provided by an OS yet."* Only pure, raw language primitives (`core`) can be used.
*   **ABI (Application Binary Interface):**
    The rules for how functions call each other at machine code level (e.g. `extern "C"`). It dictates which registers hold which arguments and who cleans up the stack.

---

## 🗺️ What Files are Involved?
1. [src/arch/boot.rs](file:///home/aj/BeanieOS/src/arch/boot.rs) — Contains the Multiboot2 header stamped into the kernel binary and the `rust_entry` function.
2. [src/main.rs](file:///home/aj/BeanieOS/src/main.rs) — Contains `kernel_main`, the true heart of our operating system.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Embedding the Multiboot2 Header Stamp
Located at lines 38–86 of [src/arch/boot.rs](file:///home/aj/BeanieOS/src/arch/boot.rs#L38-L86):

Before our kernel even runs, the bootloader needs to inspect our binary file on disk to see what features we require. We embed a `MultibootHeader` struct into a dedicated ELF section called `.multiboot_header`:

```rust
#[used]
#[unsafe(link_section = ".multiboot_header")]
static MULTIBOOT_HEADER: MultibootHeader = MultibootHeader {
    magic: 0xe85250d6, // Multiboot2 Magic
    architecture: 0,   // 0 = 32-bit (Protected Mode) entry architecture
    header_length: 80,
    checksum: ...,     // Must sum with magic and architecture to 0
    info_request: InfoRequestTag {
        typ: 1,
        flags: 0,
        size: 32,
        // We explicitly ask the bootloader for:
        // Memory map (6), Boot device (8), Command line (9),
        // Modules (1), RSDP v1/v2 for ACPI (14, 15)
        requests: [6, 8, 9, 1, 14, 15],
    },
    framebuffer_request: FramebufferRequestTag {
        typ: 5,
        flags: 0,
        size: 20,
        width: 1024,
        height: 768,
        depth: 32, // 32 bits per pixel (True Color RGB)
    },
    _pad: 0,
    end: EndTag { typ: 0, flags: 0, size: 8 },
};
```
*   **What does this achieve?**
    By declaring this static struct, the bootloader (like GRUB or Limine) scans our kernel ELF file, sees `framebuffer_request`, and automatically configures a graphical display resolution of **1024x768 with 32-bit color** before handing execution over to us!

---

### Step 2: The Rust Entry Point (`rust_entry`)
Located at lines 134–138 of [src/arch/boot.rs](file:///home/aj/BeanieOS/src/arch/boot.rs#L134-L138):

```rust
#[unsafe(no_mangle)]
pub extern "C" fn rust_entry(magic: u32, mbi_addr: u32) -> ! {
    if magic != MULTIBOOT2_MAGIC {
        halt();
    }
```
*   **`#[unsafe(no_mangle)]`**: Tells Rust: *"Do not scramble this function's name into symbols like `_ZN8beanieos...`!"* This allows our assembly code in `boot.s` to call `rust_entry` directly.
*   **`extern "C"`**: Enforces the standard C calling convention (parameters passed via registers `%rdi` and `%rsi`).
*   **`-> !`**: The "never" return type. A kernel entry function **must never return**. Where would it return to? There is no parent process to catch it! If it ever tried to return, the CPU would pop garbage from the stack and crash.
*   **The Magic Check:**
    `MULTIBOOT2_MAGIC` is `0x36d76289`. If the bootloader didn't provide this exact number in `%rdi`/`magic`, something has gone catastrophically wrong. The kernel calls `halt()`, which loops the CPU on `hlt` instructions forever.

---

### Step 3: Parsing the Bootloader's Receipt (The MBI)
Located at lines 139–148 of [src/arch/boot.rs](file:///home/aj/BeanieOS/src/arch/boot.rs#L139-L148):

```rust
    let mbi_total_size = unsafe { (mbi_addr as *const u32).read() as usize };

    let boot_info = unsafe {
        match multiboot2::BootInformation::load(
            mbi_addr as *const multiboot2::BootInformationHeader,
        ) {
            Ok(info) => info,
            Err(_) => halt(),
        }
    };
```
*   **What is `mbi_addr`?**
    It is a raw physical memory address pointing to a series of tagged blocks generated by the bootloader.
*   The first 4 bytes of this memory chunk always hold the total size in bytes of the entire data structure (`mbi_total_size`).
*   We use the `multiboot2` crate to parse this memory block. It verifies internal checksums and provides safe accessors to:
    1.  The Memory Map (which physical RAM blocks exist and which are reserved).
    2.  The Framebuffer (the video memory address where we can draw pixels).
    3.  The ACPI Tables (which allow us to discover the modern APIC interrupt controller and power management).

---

### Step 4: The Handoff to `kernel_main`
Located at line 150 of [src/arch/boot.rs](file:///home/aj/BeanieOS/src/arch/boot.rs#L150):

```rust
    kernel_main(boot_info, mbi_addr, mbi_total_size);
```
With raw hardware verified and bootloader metadata loaded safely into typed Rust data structures, the bootstrap phase is complete. Control transfers to `kernel_main` in `src/main.rs`.

---

## 🎯 Summary Checklist
By the end of Phase 2, our operating system has:
1. Verified the bootloader's secret handshake (`MULTIBOOT2_MAGIC = 0x36d76289`).
2. Read the total byte size of the boot information structure.
3. Successfully parsed the memory map and device tags into a safe Rust object (`boot_info`).
4. Entered the kernel's main orchestration function (`kernel_main`).
