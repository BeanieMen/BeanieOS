# BeanieOS Architecture & Execution Walkthrough Guide

Welcome to the internal engineering documentation for **BeanieOS**, a monolithic 64-bit x86_64 operating system written in Rust and assembly.

These guides are written to be accessible to developers with **zero prior operating system or systems programming background**. Every hardware concept, assembly register, and CPU mechanism is explained from first principles with clear analogies.

---

## 📚 Table of Contents

| Phase | Component & Subject | Document |
| :---: | :--- | :--- |
| **Overview** | High-level timeline of entire boot sequence | [timeline.md](file:///home/aj/BeanieOS/dev-notes/timeline.md) |
| **Phase 01** | Bootloader Handoff, 64-Bit Long Mode, & Identity Paging | [phase-01-boot-and-long-mode.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-01-boot-and-long-mode.md) |
| **Phase 02** | Rust Entry Point (`rust_entry`) & Multiboot2 Validation | [phase-02-rust-entry-multiboot.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-02-rust-entry-multiboot.md) |
| **Phase 03** | Physical Memory Management (PMM / Frame Allocator) | [phase-03-physical-memory.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-03-physical-memory.md) |
| **Phase 04** | Virtual Memory Management (VMM) & Dynamic Kernel Heap | [phase-04-virtual-memory-heap.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-04-virtual-memory-heap.md) |
| **Phase 05** | High-Resolution Framebuffer Graphics & Terminal Output | [phase-05-framebuffer-graphics.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-05-framebuffer-graphics.md) |
| **Phase 06** | 64-Bit GDT, Task State Segment (TSS), & IST Emergency Stacks | [phase-06-gdt-and-tss.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-06-gdt-and-tss.md) |
| **Phase 07** | Interrupt Descriptor Table (IDT) & CPU Fault Exceptions | [phase-07-idt-and-exceptions.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-07-idt-and-exceptions.md) |
| **Phase 08** | ACPI Discovery, Disabling 8259 PIC, & APIC Subsystem | [phase-08-apic-and-acpi.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-08-apic-and-acpi.md) |
| **Phase 09** | Process Management, Thread Stacks, & Assembly Context Switcher | [phase-09-processes-and-scheduler.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-09-processes-and-scheduler.md) |
| **Phase 10** | PCI Bus Scanning, AHCI (SATA) Storage Driver, & FAT Filesystem | [phase-10-pci-ahci-filesystem.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-10-pci-ahci-filesystem.md) |
| **Phase 11** | Interactive Shell, Keyboard Pipeline, & Kernel Idle Loop | [phase-11-shell-and-dispatch.md](file:///home/aj/BeanieOS/dev-notes/phases/phase-11-shell-and-dispatch.md) |
