# Monolithic 64-Bit Kernel: Execution Timeline & Walkthrough

## ⏱️ High-Level Execution Timeline
*A chronological overview of the kernel's initialization sequence.*

1. **[0.00s] Bootloader Handoff & Early Assembly:** `_start` in `src/arch/boot.s`
2. **[Phase 1] 64-Bit Long Mode Transition & Early Identity Paging:** `_start` -> `long_mode_start` in `src/arch/boot.s` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-01-boot-and-long-mode.md))
3. **[Phase 2] Rust Entry Point & Multiboot2 Information Validation:** `long_mode_start` -> `rust_entry` in `src/arch/boot.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-02-rust-entry-multiboot.md))
4. **[Phase 3] Physical Memory Management (PMM / Bump Frame Allocator):** `kernel_main` -> `Multiboot2FrameAllocator::init` in `src/memory/allocator.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-03-physical-memory.md))
5. **[Phase 4] Virtual Memory Management & Dynamic Kernel Heap Setup:** `memory::allocator::init` & `memory::heap::init_heap` in `src/memory/allocator.rs` and `src/memory/heap.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-04-virtual-memory-heap.md))
6. **[Phase 5] High-Resolution Linear Framebuffer & Console Output:** `graphics::framebuffer::init_framebuffer` in `src/graphics/framebuffer.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-05-framebuffer-graphics.md))
7. **[Phase 6] 64-Bit Global Descriptor Table (GDT) & Task State Segment (TSS):** `arch::gdt::init` in `src/arch/gdt.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-06-gdt-and-tss.md))
8. **[Phase 7] Interrupt Descriptor Table (IDT) & CPU Fault Exceptions:** `arch::interrupts::init_idt` in `src/arch/interrupts/mod.rs` & `src/arch/interrupts/faults.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-07-idt-and-exceptions.md))
9. **[Phase 8] ACPI (MADT) Discovery, Legacy 8259 PIC Disabling, & APIC Subsystem:** `arch::interrupts::pic::init` in `src/arch/interrupts/pic/mod.rs`, `madt.rs`, `lapic.rs`, `ioapic.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-08-apic-and-acpi.md))
10. **[Phase 9] Process Management & Cooperative/Preemptive Scheduler:** `task::process::init` & `task::scheduler::init` in `src/task/process.rs` & `src/task/scheduler.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-09-processes-and-scheduler.md))
11. **[Phase 10] PCI Bus Enumeration, AHCI Controller Init, & FAT Filesystem Mount:** `boot_disk` in `src/main.rs`, `src/arch/pci.rs`, `src/arch/ahci.rs`, `src/fs/mod.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-10-pci-ahci-filesystem.md))
12. **[Phase 11] Shell Process Spawning & Kernel Dispatch Loop:** `kernel_main` in `src/main.rs` ([Detailed Guide](file:///home/aj/BeanieOS/dev-notes/phases/phase-11-shell-and-dispatch.md))

---

## 🔍 Phase 1: 64-Bit Long Mode Transition & Early Identity Paging
**Triggered by:** Multiboot2 Bootloader Handoff -> **Executes:** `_start` in `src/arch/boot.s`

### What is being set up?
The CPU arrives in 32-bit protected mode. The kernel configures a minimal 4-level page table identity mapping the first 8 GiB of physical memory, enables Physical Address Extension (PAE), enables Long Mode in the Extended Feature Enable Register (EFER MSR), turns on paging, loads a temporary 64-bit Global Descriptor Table (GDT), and performs a far jump (`ljmp`) into 64-bit sub-mode (compatibility/long mode).

### How does the code set it up?
*   **Step 1: Disable Interrupts and Clear Direction Flag:** Clear interrupt flag `cli` and string copy direction `cld`:
    ```assembly
    cli
    cld
    ```
*   **Step 2: Preserve Multiboot2 Registers & Setup Early Stack:** The Multiboot2 specification passes magic in `%eax` (`0x36d76289`) and the 32-bit physical address of the Multiboot Information (MBI) structure in `%ebx`. The code saves `%eax` to `%ebp` and `%ebx` to `%esi`, then loads the early stack pointer:
    ```assembly
    mov %eax, %ebp
    mov %ebx, %esi
    mov $BOOT_STACK+65536, %esp
    ```
*   **Step 3: Construct Initial 4-Level Page Tables:** Clears 4 KiB for the P4 table (`P4_TABLE`) and 4 KiB for the P3 table (`P3_TABLE`) using `rep stosl`. Links entry 0 of P4 to `P3_TABLE` with Present and Writable bits set (`0x3`):
    ```assembly
    mov $P3_TABLE, %eax
    or $0x3, %eax               # present | writable
    mov %eax, P4_TABLE
    movl $0, P4_TABLE + 4
    ```
*   **Step 4: Map 8 GiB via 2 MiB Huge Pages in P3 & P2:** A loop links the first 8 entries of `P3_TABLE` to pre-calculated arrays of `P2_TABLES` (defined in `src/arch/boot.rs` where 8 tables of 512 entries are populated with `((k * 512 + j) * 0x20_0000) | 0x83`):
    ```assembly
    .map_p3_table:
        mov $4096, %eax
        mul %ecx
        add $P2_TABLES, %eax
        or $0x3, %eax
        mov %eax, P3_TABLE(, %ecx, 8)
        movl $0, P3_TABLE + 4(, %ecx, 8)
        inc %ecx
        cmp $8, %ecx
        jne .map_p3_table
    ```
*   **Step 5: Load CR3 and Enable PAE:** Loads `%cr3` with the base address of `P4_TABLE`, then sets bit 5 (PAE - Physical Address Extension) in `%cr4`:
    ```assembly
    mov $P4_TABLE, %eax
    mov %eax, %cr3
    mov %cr4, %eax
    or $(1 << 5), %eax          # CR4.PAE
    mov %eax, %cr4
    ```
*   **Step 6: Enable Long Mode in EFER MSR:** Reads Model-Specific Register `0xC000_0080` (IA32_EFER) via `rdmsr`, sets bit 8 (LME - Long Mode Enable), and writes it back via `wrmsr`:
    ```assembly
    mov $0xc0000080, %ecx       # EFER MSR
    rdmsr
    or $(1 << 8), %eax          # EFER.LME
    wrmsr
    ```
*   **Step 7: Activate Paging and Enter Compatibility Mode:** Enables bit 31 (`PG`) and bit 0 (`PE`) in `%cr0`:
    ```assembly
    mov %cr0, %eax
    or $(1 << 31 | 1), %eax     # CR0.PG | CR0.PE
    mov %eax, %cr0
    ```
*   **Step 8: Load 64-bit GDT and Execute Long Jump:** Loads temporary 64-bit GDT descriptor `gdt64_pointer` and executes a far jump into `long_mode_start` using code segment selector `0x08`:
    ```assembly
    lgdt gdt64_pointer
    ljmp $0x08, $long_mode_start
    ```
*   **Step 9: Reload 64-Bit Segment Registers & Establish 64-Bit Calling Convention:** In `.code64`, reloads data segment registers (`ds, es, fs, gs, ss`) with data selector `0x10`, reloads `%rsp` with the 64-bit boot stack address, places the Multiboot2 magic in `%rdi` (1st parameter) and MBI address in `%rsi` (2nd parameter) per System V AMD64 ABI, and branches to Rust:
    ```assembly
    mov $0x10, %ax
    mov %ax, %ds ...
    mov $BOOT_STACK+65536, %rsp
    mov %ebp, %edi
    mov %esi, %esi
    call rust_entry
    ```

### Why is this needed?
x86_64 CPUs cannot transition directly from 16-bit real mode or 32-bit protected mode into 64-bit long mode without active 4-level paging and PAE enabled. Setting `EFER.LME` merely primes the CPU; long mode only activates once `CR0.PG` is asserted with a valid 4-level paging structure in `CR3`. The far jump (`ljmp`) is required to flush the CPU instruction prefetch pipeline and update the Code Segment (`CS`) descriptor cache to a 64-bit code segment (with the `L` bit set in the descriptor).

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: Setting Up Long Mode](https://wiki.osdev.org/Setting_Up_Long_Mode)
*   🔗 [Intel SDM Volume 3A, Chapter 9.8.5: Initializing IA-32e Mode](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html) - *Specifies the mandatory register programming sequence: CR4.PAE -> EFER.LME -> CR0.PG -> Far Jump.*
*   🔗 [AMD64 Architecture Programmer's Manual Volume 2, Chapter 14: Processor Initialization to Long Mode](https://www.amd.com/content/dam/amd/en/documents/processor-tech-docs/programmer-references/24593.pdf)

---

## 🔍 Phase 2: Rust Entry Point & Multiboot2 Information Validation
**Triggered by:** `long_mode_start` in `src/arch/boot.s` -> **Executes:** `rust_entry` in `src/arch/boot.rs`

### What is being set up?
The initial boundary between raw assembly and Rust code. It verifies the bootloader handshake contract, extracts total Multiboot2 memory structure sizing, parses the Multiboot2 tags, and delegates control to `kernel_main`.

### How does the code set it up?
*   **Step 1: Check Multiboot2 Magic:** Asserts that the first argument `magic` matches `0x36d76289`. If invalid, the CPU is immediately halted:
    ```rust
    if magic != MULTIBOOT2_MAGIC {
        halt();
    }
    ```
*   **Step 2: Read Multiboot2 Header Size:** Reads the first 32-bit word at `mbi_addr` to obtain the total size in bytes of the multiboot information container:
    ```rust
    let mbi_total_size = unsafe { (mbi_addr as *const u32).read() as usize };
    ```
*   **Step 3: Parse Multiboot2 Information Structure:** Uses `multiboot2::BootInformation::load` to parse the tagged structure at `mbi_addr`:
    ```rust
    let boot_info = unsafe {
        match multiboot2::BootInformation::load(
            mbi_addr as *const multiboot2::BootInformationHeader,
        ) {
            Ok(info) => info,
            Err(_) => halt(),
        }
    };
    ```
*   **Step 4: Transition to Kernel Main:** Invokes `kernel_main(boot_info, mbi_addr, mbi_total_size)` in `src/main.rs`.

### Why is this needed?
The bootloader provides critical hardware layout metadata: memory maps (usable RAM vs reserved firmware areas), linear framebuffer configuration, and ACPI pointers (RSDP). Without validating `MULTIBOOT2_MAGIC`, the kernel risks executing with invalid memory pointers.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [Multiboot2 Specification Version 2.0](https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html) - *Details the memory tag structures, magic constants (`0x36d76289`), and MBI layout.*
*   🔗 [OSDev Wiki: Multiboot2](https://wiki.osdev.org/Multiboot_2)

---

## 🔍 Phase 3: Physical Memory Management (PMM / Bump Frame Allocator)
**Triggered by:** `kernel_main` in `src/main.rs` -> **Executes:** `memory::allocator::Multiboot2FrameAllocator::init` in `src/memory/allocator.rs`

### What is being set up?
The physical page frame allocator (`Multiboot2FrameAllocator`), responsible for tracking free 4 KiB physical memory frames from the BIOS/UEFI E820 memory map while strictly reserving low firmware memory, the kernel binary image, and the Multiboot2 information structure.

### How does the code set it up?
*   **Step 1: Extract Memory Map and Kernel Bounds:** Queries the `memory_map_tag()` from `boot_info` and queries linker-generated symbols `kernel_start` and `kernel_end`:
    ```rust
    let memory_map = boot_info.memory_map_tag().expect("No Multiboot2 memory map");
    let (kstart, kend) = kernel_range();
    ```
*   **Step 2: Find First Usable Memory Area:** Iterates through `MemoryArea` descriptors, filters by `MemoryAreaType::Available`, and aligns the base address to a 4 KiB boundary:
    ```rust
    for (i, area) in areas.iter().enumerate() {
        if area.typ() != MemoryAreaType::Available { continue; }
        let addr = align_up(area.start_address());
        if addr + PAGE <= area.end_address() {
            area_idx = i;
            curr_addr = addr;
            break;
        }
    }
    ```
*   **Step 3: Implement Frame Reservation Checks:** During frame allocation via `allocate_frame()`, frames are verified against reserved hardware and software boundaries:
    ```rust
    fn frame_is_reserved(addr: u64, kstart: u64, kend: u64, mbi_start: u64, mbi_end: u64) -> bool {
        let frame_end = addr + PAGE;
        if addr < 0x10_0000 { return true; } // Real mode IVT, BDA, EBDA, ROM
        if addr < kend && frame_end > kstart { return true; } // Kernel image
        if addr < mbi_end && frame_end > mbi_start { return true; } // Multiboot2 data
        false
    }
    ```

### Why is this needed?
In a 64-bit OS, virtual memory pages must be backed by distinct physical memory frames. If the physical allocator gives out addresses occupied by the kernel's code/data, the IVT, or the Multiboot2 structure, the kernel will overwrite itself, leading to corrupt execution or catastrophic triple faults.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: Page Frame Allocation](https://wiki.osdev.org/Page_Frame_Allocation)
*   🔗 [Intel SDM Volume 3A, Chapter 4.1: Paging Overview](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html) - *Specifies the 4 KiB physical address alignment constraints for page directories and tables.*

---

## 🔍 Phase 4: Virtual Memory Management & Dynamic Kernel Heap Setup
**Triggered by:** `kernel_main` in `src/main.rs` -> **Executes:** `memory::allocator::init` & `memory::heap::init_heap` in `src/memory/allocator.rs` and `src/memory/heap.rs`

### What is being set up?
A higher-level page table mapper (`OffsetPageTable`) that manipulates active 4-level page tables, maps a contiguous 1000 KiB virtual memory range starting at `0x4444_4444_0000`, and initializes the global kernel allocator (`linked_list_allocator::LockedHeap`) enabling Rust heap abstractions (`Vec`, `Box`, `Arc`, `BTreeMap`).

### How does the code set it up?
*   **Step 1: Obtain Active Level 4 Page Table Pointer:** Reads the physical address stored in `%cr3` and converts it to a virtual pointer using the identity mapping offset (`VirtAddr::new(0)`):
    ```rust
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    ```
*   **Step 2: Map Heap Virtual Pages to Physical Frames:** Iterates over the virtual range `[0x4444_4444_0000 .. 0x4444_4444_0000 + 1000 KiB]`. For each 4 KiB page, allocates a physical frame from `frame_allocator` and inserts the mapping into the page tables:
    ```rust
    for page in page_range {
        let frame = frame_allocator.allocate_frame().ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }
    ```
*   **Step 3: Initialize Global Heap Allocator:** Hands the mapped virtual memory span to the global allocator:
    ```rust
    unsafe { ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE) };
    ```

### Why is this needed?
The Rust core library (`alloc`) requires a global allocator to support variable-sized data structures. Architecturally, mapping virtual memory into level 4, 3, 2, and 1 page tables enforces hardware protection attributes (`PRESENT`, `WRITABLE`, `NO_EXECUTE`) and provides isolated addressing spaces.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: Paging](https://wiki.osdev.org/Paging)
*   🔗 [Intel SDM Volume 3A, Chapter 4.5: 4-Level Paging and 5-Level Paging](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html) - *Specifies entry layouts for PML4, PDPT, PD, and PT structures.*

---

## 🔍 Phase 5: High-Resolution Linear Framebuffer & Console Output
**Triggered by:** `kernel_main` in `src/main.rs` -> **Executes:** `graphics::framebuffer::init_framebuffer` in `src/graphics/framebuffer.rs`

### What is being set up?
Initializes the linear graphics framebuffer (provided by UEFI/GOP or VESA BIOS through Multiboot2), configures the global `WRITER`, zeroes video memory, and enables terminal text rendering via an embedded 8x16 bitmap font.

### How does the code set it up?
*   **Step 1: Retrieve Multiboot2 Framebuffer Tag:** Obtains linear physical address, width, height, pitch (bytes per scanline), and bits per pixel (bpp):
    ```rust
    let fb_tag = boot_info.framebuffer_tag().unwrap().unwrap();
    graphics::framebuffer::init_framebuffer(
        fb_tag.address(), fb_tag.width(), fb_tag.height(), fb_tag.pitch(), fb_tag.bpp(),
    );
    ```
*   **Step 2: Clear Framebuffer Memory:** Clears video memory directly via pointer write:
    ```rust
    core::ptr::write_bytes(addr, 0, (height * pitch) as usize);
    ```
*   **Step 3: Text Font Glyph Blitting & Hardware Scrolling:** Characters are written row-by-row based on `FONT` (an 8x16 glyph table). When text exceeds vertical bounds, `scroll()` copies pixel memory upwards and clears the bottom row:
    ```rust
    core::ptr::copy(fb.addr.add(shift), fb.addr, text_bytes - shift);
    core::ptr::write_bytes(fb.addr.add(text_bytes - shift), 0, shift);
    ```

### Why is this needed?
In 64-bit UEFI or modern PC platforms, legacy VGA text-mode memory at `0xB8000` is either unavailable, unmapped, or non-functional. Operating a linear graphics framebuffer is required for debugging messages and user shell interaction.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: GOP (Graphics Output Protocol)](https://wiki.osdev.org/GOP)
*   🔗 [OSDev Wiki: Drawing In a Linear Framebuffer](https://wiki.osdev.org/Drawing_In_a_Linear_Framebuffer)

---

## 🔍 Phase 6: 64-Bit Global Descriptor Table (GDT) & Task State Segment (TSS)
**Triggered by:** `kernel_main` in `src/main.rs` -> **Executes:** `arch::gdt::init` in `src/arch/gdt.rs`

### What is being set up?
Replaces the early bootstrap GDT with a fully featured 64-bit Global Descriptor Table containing a 64-bit kernel code segment, a 64-bit Task State Segment (TSS) descriptor with an Interrupt Stack Table (IST) entry for Double Faults, and sets segment registers (`CS, DS, ES, FS, GS, SS`) and the Task Register (`TR`).

### How does the code set it up?
*   **Step 1: Allocate Dedicated Double Fault IST Stack:** Allocates a 20 KiB dedicated stack buffer in `.bss` and assigns its top address to IST index 0:
    ```rust
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
        const STACK_SIZE: usize = 4096 * 5;
        static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
        let stack_start = VirtAddr::from_ptr(&raw const STACK);
        stack_start + STACK_SIZE as u64
    };
    ```
*   **Step 2: Construct GDT:** Appends a 64-bit kernel code descriptor and a 16-byte TSS descriptor into the GDT structure:
    ```rust
    let mut gdt = GlobalDescriptorTable::new();
    let code_selector = gdt.append(Descriptor::kernel_code_segment());
    let tss_selector = gdt.append(Descriptor::tss_segment(&TSS));
    ```
*   **Step 3: Load GDT and Reload Segment Registers:** Invokes the `lgdt` instruction, reloads `CS` via far return, sets data segment selectors to null (`SegmentSelector(0)`), and loads the Task Register via `ltr`:
    ```rust
    GDT.0.load();
    CS::set_reg(GDT.1.code_selector);
    DS::set_reg(SegmentSelector(0));
    ES::set_reg(SegmentSelector(0));
    FS::set_reg(SegmentSelector(0));
    GS::set_reg(SegmentSelector(0));
    SS::set_reg(SegmentSelector(0));
    load_tss(GDT.1.tss_selector);
    ```

### Why is this needed?
In 64-bit mode, memory segmentation is largely disabled, but the GDT remains mandatory for defining the CPU privilege level (Ring 0 vs Ring 3), Code Segment long mode attributes (`L` bit, `D` bit), and loading the TSS. The TSS is essential because hardware task switching is removed in x86_64, but the TSS still stores the Interrupt Stack Table (IST). If a kernel stack overflows, invoking a Double Fault handler on that same corrupted stack causes an immediate triple fault. The IST guarantees that the CPU switches to a known clean stack.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: Global Descriptor Table](https://wiki.osdev.org/Global_Descriptor_Table)
*   🔗 [OSDev Wiki: Task State Segment](https://wiki.osdev.org/Task_State_Segment)
*   🔗 [Intel SDM Volume 3A, Chapter 3.4.5: Segment Descriptors & Chapter 7.7: Task Management in 64-bit Mode](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)

---

## 🔍 Phase 7: Interrupt Descriptor Table (IDT) & CPU Fault Exceptions
**Triggered by:** `kernel_main` in `src/main.rs` -> **Executes:** `arch::interrupts::init_idt` in `src/arch/interrupts/mod.rs` & `src/arch/interrupts/faults.rs`

### What is being set up?
Initializes a 256-entry 64-bit Interrupt Descriptor Table (IDT) mapping CPU exception vectors (0–31), binds the Double Fault exception to IST index 0, registers hardware interrupt handlers (Timer, Keyboard, Spurious), and loads the IDT pointer into the CPU's IDTR register via `lidt`.

### How does the code set it up?
*   **Step 1: Register CPU Fault Exception Handlers:** Registers 64-bit interrupt gate handlers for vectors 0 through 21 (Divide Error, Invalid Opcode, General Protection Fault, Page Fault, etc.):
    ```rust
    idt.divide_error.set_handler_fn(divide_error_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.general_protection_fault.set_handler_fn(general_protection_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);
    ```
*   **Step 2: Bind Double Fault to TSS IST Stack:** Configures vector 8 (Double Fault) to use the dedicated IST stack:
    ```rust
    idt.double_fault
        .set_handler_fn(double_fault_handler)
        .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    ```
*   **Step 3: Register Hardware Device Vectors:** Maps Vector 32 to LAPIC Timer, Vector 33 to PS/2 Keyboard, and Vector 255 to Spurious Interrupts:
    ```rust
    idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_interrupt_handler);
    idt[SPURIOUS_VECTOR].set_handler_fn(spurious_interrupt_handler);
    idt[LAPIC_TIMER_VECTOR].set_handler_fn(timer_interrupt_handler);
    ```
*   **Step 4: Load IDT:** Invokes `lidt`:
    ```rust
    IDT.load();
    ```

### Why is this needed?
The CPU relies on the IDT to dispatch software exceptions and external hardware interrupts. In 64-bit mode, IDT gate descriptors are 16 bytes wide (holding a 64-bit handler address, segment selector, IST index, and flags). Without a valid IDT, any divide-by-zero, invalid opcode, or page fault will immediately escalate to a double fault and triple fault, resetting the machine.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: Interrupt Descriptor Table](https://wiki.osdev.org/Interrupt_Descriptor_Table)
*   🔗 [Intel SDM Volume 3A, Chapter 6.14: Exception and Interrupt Handling in 64-bit Mode](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html) - *Specifies the 16-byte gate descriptor layout and IST mechanism.*

---

## 🔍 Phase 8: ACPI Discovery, Legacy 8259 PIC Disabling, & APIC Subsystem
**Triggered by:** `arch::interrupts::init_idt` in `src/arch/interrupts/mod.rs` -> **Executes:** `arch::interrupts::pic::init` in `src/arch/interrupts/pic/mod.rs`

### What is being set up?
Locates ACPI tables via RSDP (Root System Description Pointer) and XSDT/RSDT, parses the MADT (Multiple APIC Description Table), masks and disables the legacy 8259 dual PIC, enables and programs the Local APIC (LAPIC) and its timer, routes the PS/2 keyboard through the I/O APIC, and executes `sti` to enable global CPU interrupts.

### How does the code set it up?
*   **Step 1: Traverse XSDT/RSDT to Locate MADT:** Identifies the table with signature `"APIC"`:
    ```rust
    let signature = core::slice::from_raw_parts(table as *const u8, 4);
    if signature == b"APIC" { return table; }
    ```
*   **Step 2: Parse MADT Structures:** Reads the Local APIC MMIO physical base address (offset +36), locates I/O APIC entries (type 1), and checks for Interrupt Source Overrides (type 2, remapping legacy ISA IRQ1 to a target Global System Interrupt / GSI):
    ```rust
    match typ {
        1 => { // I/O APIC
            let address = read_u32(offset + 4) as usize;
            let gsi_base = read_u32(offset + 8);
            ioapic_address = Some(address);
            ioapic_gsi_base = Some(gsi_base);
        }
        2 => { // Interrupt Source Override
            let bus = read_u8(offset + 2);
            let source = read_u8(offset + 3);
            let gsi = read_u32(offset + 4);
            if bus == 0 && source == 1 { keyboard_gsi = gsi; }
        }
    }
    ```
*   **Step 3: Mask Legacy 8259 PIC:** Writes `0xFF` to legacy I/O ports `0x21` and `0xA1` to prevent ghost interrupts from firing on unmapped vectors:
    ```rust
    Port::new(0x21).write(0xFFu8);
    Port::new(0xA1).write(0xFFu8);
    ```
*   **Step 4: Enable Local APIC (LAPIC):** Reads the Spurious Vector Register (SVR), sets bit 8 (APIC Software Enable), and sets the spurious vector to `0xFF`:
    ```rust
    let svr = lapic_svr_reg() as *mut u32;
    let value = core::ptr::read_volatile(svr);
    core::ptr::write_volatile(svr, value | LAPIC_SVR_ENABLE | SPURIOUS_VECTOR as u32);
    ```
*   **Step 5: Configure Periodic LAPIC Timer:** Configures divide value to 16, sets timer LVT to Periodic mode on Vector 32 (`0x20`), and writes initial count `0x100_000`:
    ```rust
    write_volatile(lapic_timer_divide_reg() as *mut u32, LAPIC_TIMER_DIVIDE_16);
    write_volatile(lapic_timer_reg() as *mut u32, LAPIC_TIMER_PERIODIC | LAPIC_TIMER_VECTOR as u32);
    write_volatile(lapic_timer_initial_reg() as *mut u32, initial_count);
    ```
*   **Step 6: Configure I/O APIC Redirection Table for Keyboard:** Computes the redirection register pair for the keyboard GSI (`0x10 + index * 2`), targets the LAPIC ID in the high 32 bits, and programs `KEYBOARD_VECTOR` (33) in the low 32 bits:
    ```rust
    ioapic_write(address, high as u8, lapic_id << 24);
    ioapic_write(address, low as u8, KEYBOARD_VECTOR as u32);
    ```
*   **Step 7: Enable Interrupts:** Unmasks maskable external interrupts at the CPU:
    ```rust
    x86_64::instructions::interrupts::enable(); // sti
    ```

### Why is this needed?
The 8259 PIC is a legacy 16-bit AT-architecture controller limited to 15 interrupt lines, lacks SMP support, and overlaps with CPU exception vectors by default. Modern x86_64 architectures require the Advanced Programmable Interrupt Controller (APIC) architecture consisting of a per-core Local APIC and chipset I/O APICs to handle bus-mastered MSIs, multi-core IPIs, and high-frequency timers.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: MADT](https://wiki.osdev.org/MADT)
*   🔗 [OSDev Wiki: APIC](https://wiki.osdev.org/APIC)
*   🔗 [OSDev Wiki: IOAPIC](https://wiki.osdev.org/IOAPIC)
*   🔗 [Intel SDM Volume 3A, Chapter 10: Advanced Programmable Interrupt Controller (APIC)](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)

---

## 🔍 Phase 9: Process Management & Multitasking Scheduler Initialization
**Triggered by:** `kernel_main` in `src/main.rs` -> **Executes:** `task::process::init` & `task::scheduler::init` in `src/task/process.rs` & `src/task/scheduler.rs`

### What is being set up?
Establishes process isolation bookkeeping (`ProcessManager`), creates the primary kernel process (PID 0) and the primary kernel execution thread (TID 0), initializes priority-based multi-queue round-robin ready queues, and establishes the assembly context switching mechanism (`switch_context`).

### How does the code set it up?
*   **Step 1: Initialize Kernel Process Container:** Instantiates PID 0 as the root container:
    ```rust
    let kernel_proc = Arc::new(Mutex::new(Process::new_kernel()));
    self.processes.insert(ProcessId::KERNEL, kernel_proc);
    self.current_pid = ProcessId::KERNEL;
    ```
*   **Step 2: Initialize Main Kernel Thread:** Registers TID 0 as the currently executing context:
    ```rust
    let main = Arc::new(Mutex::new(Thread::new_main()));
    self.threads.insert(ThreadId::MAIN, main);
    self.current = ThreadId::MAIN;
    ```
*   **Step 3: Setup Callee-Saved Register Frame for Context Switching:** New threads allocate a dedicated 64 KiB stack and format the top of the stack to mimic a suspended context with return address `entry`, exit trampoline, and preserved registers:
    ```rust
    sp -= 8;
    *(sp as *mut usize) = thread_trampoline_exit as usize;
    sp -= 8;
    *(sp as *mut usize) = entry as usize;
    sp -= 7 * 8; // rflags, r15, r14, r13, r12, rbx, rbp
    core::ptr::write_bytes(sp as *mut u8, 0, 7 * 8);
    let rflags_ptr = sp.wrapping_add(6 * 8) as *mut usize;
    *rflags_ptr = 0x200; // IF enabled (bit 9)
    ```
*   **Step 4: Assembly Context Switch Routine:** Implements `switch_context` using `naked_asm!` to save old registers to `*old_rsp` and restore new registers from `new_rsp`:
    ```assembly
    pushfq
    push r15; push r14; push r13; push r12; push rbx; push rbp
    mov [rdi], rsp
    mov rsp, rsi
    pop rbp; pop rbx; pop r12; pop r13; pop r14; pop r15
    popfq
    ret
    ```

### Why is this needed?
A monolithic kernel requires abstraction primitives to track processes (memory and resource containers) and threads (independent execution streams). Context switching preserves the caller/callee register state and `RFLAGS` so threads can yield or sleep transparently without clobbering CPU state.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: Context Switching](https://wiki.osdev.org/Context_Switching)
*   🔗 [System V Application Binary Interface AMD64 Architecture Processor Supplement](https://gitlab.com/x86-psABIs/x86-64-ABI) - *Specifies callee-saved registers (`rbx, rsp, rbp, r12, r13, r14, r15`) and 16-byte stack alignment requirements.*

---

## 🔍 Phase 10: PCI Bus Enumeration, AHCI Controller Init, & FAT Filesystem Mount
**Triggered by:** `kernel_main` in `src/main.rs` -> **Executes:** `boot_disk` in `src/main.rs`, `src/arch/pci.rs`, `src/arch/ahci.rs`, `src/fs/mod.rs`

### What is being set up?
Probes the PCI bus via Configuration Mechanism #1 (`0xCF8`/`0xCFC`) for AHCI SATA storage controllers, resets and initializes the AHCI HBA (Host Bus Adapter), sets up Command Lists and PRDT (Physical Region Descriptor Tables) for direct memory access (DMA), checks sector 0/1 read capability, and mounts the FAT filesystem on the primary partition.

### How does the code set it up?
*   **Step 1: Enumerate PCI Devices:** Scans buses, devices, and functions looking for Class `0x01` (Mass Storage), Subclass `0x06` (SATA), Interface `0x01` (AHCI):
    ```rust
    let devices = arch::pci::find_ahci();
    ```
*   **Step 2: Read BAR5 (ABAR):** Extracts the 32-bit or 64-bit AHCI Base Address Register:
    ```rust
    let (bar5, size) = device.bar5_info().ok_or("no usable BAR5")?;
    ```
*   **Step 3: Reset HBA and Enable AHCI Mode:** Asserts `GHC.HR` (HBA Reset) and waits for completion, then sets `GHC.AE` (AHCI Enable):
    ```rust
    write_volatile(ghc_ptr, read_volatile(ghc_ptr) | GHC_HR);
    write_volatile(ghc_ptr, read_volatile(ghc_ptr) | GHC_AE);
    ```
*   **Step 4: Configure Port DMA Structures:** Locates an active port with PHY communication established (`PxSSTS.DET == 0x3`), programs `PxCLB` (Command List Base) and `PxFB` (FIS Base):
    ```rust
    write_volatile(port.add(PxCLB), &raw const COMMAND_LIST as u32);
    write_volatile(port.add(PxFB), &raw const RECEIVE_AREA as u32);
    ```
*   **Step 5: Identify Drive and Read Test Sector:** Issues an `ATA_IDENTIFY` Command FIS to determine geometry and sector count, then verifies read pipeline by reading sector 1:
    ```rust
    controller.read_at(512, &mut first)?;
    ```
*   **Step 6: Mount FAT Filesystem:** Wraps the AHCI controller in a `Disk` block device and mounts the filesystem via `fatfs::FileSystem::new`:
    ```rust
    let filesystem = fs::mount(controller)?;
    ```

### Why is this needed?
Access to non-volatile secondary storage (SATA/SSD) is required for persistent logging, user binaries, system configuration, and dynamic module loading. AHCI standardizes SATA programming across modern x86 hardware.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: PCI](https://wiki.osdev.org/PCI)
*   🔗 [OSDev Wiki: AHCI](https://wiki.osdev.org/AHCI)
*   🔗 [Serial ATA AHCI 1.3.1 Specification](https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/serial-ata-ahci-spec-rev1-3-1.pdf)

---

## 🔍 Phase 11: Shell Process Spawning & Kernel Dispatch Loop
**Triggered by:** `kernel_main` in `src/main.rs` -> **Executes:** `task::scheduler::spawn_in_process` in `src/main.rs`

### What is being set up?
Spawns the interactive `shell` worker thread and an auxiliary graphical animation thread (`rgb_square`), wraps the mounted disk in the global `SHELL` container, and drops the main kernel thread into an idle dispatch loop.

### How does the code set it up?
*   **Step 1: Instantiate Shell with Mounted Disk:**
    ```rust
    SHELL.call_once(|| Mutex::new(Shell::new(disk)));
    ```
*   **Step 2: Create Processes and Spawn Threads:**
    ```rust
    let shell_pid = task::process::create_process("shell", "/");
    task::scheduler::spawn_in_process(
        shell_pid, "shell", shell_task, task::scheduler::Priority::Normal,
    );

    let rgb_pid = task::process::create_process("rgb_square", "/");
    task::scheduler::spawn_in_process(
        rgb_pid, "rgb_square", rgb_square_task, task::scheduler::Priority::Normal,
    );
    ```
*   **Step 3: Kernel Idle & Yield Loop:** The initial bootstrap thread yields CPU time slices to ready tasks and enters a halted state when no tasks need immediate execution:
    ```rust
    loop {
        task::scheduler::yield_now();
        x86_64::instructions::hlt();
    }
    ```

### Why is this needed?
Separating the interactive shell and display rendering into discrete threads validates that the multi-queue scheduler, interrupt-driven keyboard queue (`push_key`/`pop_key`), and memory subsystem function harmoniously in an asynchronous environment.

### Documentation & Specifications
*How it should be implemented according to spec:*
*   🔗 [OSDev Wiki: Scheduling Algorithms](https://wiki.osdev.org/Scheduling_Algorithms)
*   🔗 [Intel SDM Volume 2A, Instruction Reference: HLT](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html) - *Explains CPU halting and low-power standby until the next unmasked interrupt.*

---

## 🛠️ Potential Gaps & Architectural Deviations
*An analysis of anything missing from the codebase that is typically required for a fully compliant 64-bit monolithic kernel, or areas where the code deviates from standard hardware specifications.*

*   **Missing Cache Invalidation and Uncacheable Flags on MMIO Regions:**
    *   *Issue:* The Local APIC (`0xFEE0_0000`), I/O APIC (`0xFEC0_0000`), PCI BAR5 AHCI registers, and the UEFI linear framebuffer are accessed directly via raw physical pointers. These addresses fall within the 0..8 GiB range mapped during early boot (`src/arch/boot.s`) with default cacheable flags (`PageTableFlags::PRESENT | PageTableFlags::WRITABLE`).
    *   *Architectural Impact:* In real x86_64 hardware (unlike QEMU), memory-mapped I/O (MMIO) registers **must** be mapped as Strong Uncacheable (`CacheDisable` / `PageTableFlags::NO_CACHE`) or Write-Combining (for framebuffers via PAT). Mapping MMIO as normal write-back cacheable RAM causes CPU caches to cache register reads and buffer register writes out-of-order, causing missed interrupts, corrupted AHCI DMA command dispatches, or bus freezes.
*   **Omission of FPU / SSE / AVX Control Register Configuration:**
    *   *Issue:* In `_start` (`src/arch/boot.s`), the kernel enters long mode without configuring `CR0.EM`, `CR0.MP`, `CR4.OSFXSR`, or `CR4.OSXMMEXCPT`.
    *   *Architectural Impact:* The 64-bit System V ABI mandates SSE2 support. The Rust compiler frequently emits SSE instructions (`movups`, `movaps`, `xorps`) for basic operations such as zeroing arrays or struct copies. Without clearing `CR0.EM` and setting `CR4.OSFXSR`, the CPU will trigger an Invalid Opcode (`#UD`) or Device Not Available (`#NM`) fault on the first floating-point or SSE vector instruction.
*   **Timer Interrupt Does Not Preempt Running Tasks (Cooperative Only):**
    *   *Issue:* In `src/arch/interrupts/vectors.rs`, `timer_interrupt_handler` merely increments `TICKS` and sends an EOI to the Local APIC. It does not invoke `task::scheduler::yield_now()`.
    *   *Architectural Impact:* Multitasking is currently entirely cooperative. If a task enters a compute loop (or a user command hangs), the system freezes because the hardware timer interrupt returns directly back to the interrupted instruction without initiating a context switch.
*   **FPU/SSE State Not Saved During Context Switching:**
    *   *Issue:* `switch_context` in `src/task/scheduler.rs` only pushes and pops general-purpose registers (`r15..r12, rbx, rbp, rflags`).
    *   *Architectural Impact:* If multiple threads perform floating-point or SIMD math (such as graphics rasterization in `rgb_square_task`), their XMM/YMM registers will silently corrupt each other. Standard kernels save/restore FPU state using `fxsave64`/`fxrstor64` or `xsave`/`xrstor`.
*   **Missing TSS Privilege Stack Table (RSP0) for Ring 3 User Mode:**
    *   *Issue:* `src/arch/gdt.rs` sets up an Interrupt Stack Table (`IST0`) for Double Faults, but leaves `privilege_stack_table[0]` (`RSP0`) empty.
    *   *Architectural Impact:* When implementing user-space processes (Ring 3), any interrupt, exception, or syscall transitioning from Ring 3 to Ring 0 will automatically attempt to load the kernel stack from `TSS.RSP0`. If uninitialized, stack operations will fault at address `0x0`, causing an immediate double fault.
*   **Unsynchronized Static DMA Buffers in AHCI Driver:**
    *   *Issue:* `COMMAND_LIST`, `COMMAND_TABLE`, and `TRANSFER_BUFFER` in `src/arch/ahci.rs` are declared as mutable `static` globals without spinlocks or thread synchronization.
    *   *Architectural Impact:* If multiple threads issue concurrent file I/O operations, they will overwrite each other's DMA command headers and transfer buffers, corrupting disk data.
