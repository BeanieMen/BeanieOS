# Phase 3: Physical Memory Management (PMM) & Frame Allocation

## 🌟 High-Level Overview
Imagine your computer's RAM as a massive parking lot with millions of parking spots. Each parking spot is exactly **4,096 bytes (4 KiB)** in size. In systems programming, these 4 KiB parking spots are called **Physical Frames**.

However, you cannot just let any program park anywhere! Some spots have buildings on them (like the motherboard's firmware ROM, or video memory). Other spots are already occupied by our own operating system's executable code. If we accidentally park new data on top of our own kernel code, the system will immediately self-destruct.

In this phase, we build the **Physical Memory Manager (PMM)**. It scans the hardware map provided by the computer's motherboard, filters out all dangerous and reserved zones, and hands out free 4 KiB physical frames one by one upon request.

---

## 📖 Layman's Glossary: Jargon Demystified

*   **Physical Memory (RAM):**
    The actual physical silicon microchips plugged into the motherboard. Each byte has a true, physical electrical wire address (e.g., `0x0010_0000`).
*   **Physical Frame:**
    The fundamental currency of modern x86 memory. The CPU divides physical RAM into fixed-size chunks of **4,096 bytes (4 KiB)**. A frame is a 4 KiB bucket in real physical RAM.
*   **Virtual Page:**
    A 4 KiB bucket of *imaginary* address space used by software. Later on (in Phase 4), we will glue Virtual Pages to Physical Frames using Page Tables.
*   **The E820 Memory Map:**
    When a computer boots, the BIOS/UEFI surveys the motherboard to find where RAM is installed, which parts are broken, and which parts are reserved for hardware devices. It writes this survey down in a table called the "memory map" and passes it to the bootloader.
*   **Reserved Memory Regions:**
    *   **The First 1 Megabyte (`0x0 .. 0x10_0000`):** Ancient IBM PC hardware territory. Contains the Real Mode Interrupt Vector Table (IVT), BIOS Data Area (BDA), Extended BIOS Data Area (EBDA), and Video Graphics memory. **Never touch this!**
    *   **Kernel Image:** Where our compiled binary code (`.text`, `.rodata`, `.data`, `.bss`) sits in RAM.
    *   **Multiboot2 Information (MBI):** The data packet the bootloader just handed us. If we overwrite it, we lose our hardware information!
*   **Bump Allocator (Watermark Allocator):**
    The simplest and fastest type of memory allocator. It keeps an integer index (the "cursor") pointing to the next available address. When someone asks for a frame, it hands over the current address and "bumps" the cursor up by 4,096 bytes.

---

## 🗺️ What Files are Involved?
1. [src/memory/allocator.rs](file:///home/aj/BeanieOS/src/memory/allocator.rs) — Contains `Multiboot2FrameAllocator` and the physical memory reservation logic.
2. [linker.ld](file:///home/aj/BeanieOS/linker.ld) — Defines the memory markers `kernel_start` and `kernel_end`.
3. [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L137-L143) — Initializes the allocator with the parsed Multiboot2 memory map.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Learning Where the Kernel Lives in RAM
Located at lines 10–28 of [src/memory/allocator.rs](file:///home/aj/BeanieOS/src/memory/allocator.rs#L10-L28):

Our linker script ([linker.ld](file:///home/aj/BeanieOS/linker.ld)) puts our kernel at the 1 Megabyte mark (`1M` = `0x100000`) and stamps two boundary symbols around it:
```ld
    . = 1M;
    kernel_start = .;
    /* ... all code sections ... */
    kernel_end = .;
```
In Rust, we import these symbols to find out our exact starting and ending addresses in physical RAM:
```rust
unsafe extern "C" {
    static kernel_start: u8;
    static kernel_end: u8;
}

fn kernel_range() -> (u64, u64) {
    unsafe {
        let start = (&kernel_start as *const u8) as u64;
        let end = (&kernel_end as *const u8) as u64;
        (start, end)
    }
}
```

---

### Step 2: Guarding the Forbidden Zones
Located at lines 30–45 of [src/memory/allocator.rs](file:///home/aj/BeanieOS/src/memory/allocator.rs#L30-L45):

Whenever the allocator wants to hand out a physical frame, it must verify that the frame does not collide with critical data:
```rust
fn frame_is_reserved(addr: u64, kstart: u64, kend: u64, mbi_start: u64, mbi_end: u64) -> bool {
    let frame_end = addr + PAGE; // PAGE = 4096 bytes

    // 1. Below 1 MiB is strictly reserved for motherboard firmware
    if addr < 0x10_0000 {
        return true;
    }
    // 2. Inside the kernel binary's footprint
    if addr < kend && frame_end > kstart {
        return true;
    }
    // 3. Inside the Multiboot2 info data structure
    if addr < mbi_end && frame_end > mbi_start {
        return true;
    }
    false
}
```

---

### Step 3: Initializing the Frame Allocator
Located at lines 48–85 of [src/memory/allocator.rs](file:///home/aj/BeanieOS/src/memory/allocator.rs#L48-L85):

```rust
pub struct Multiboot2FrameAllocator<'a> {
    areas: &'a [MemoryArea],
    area_idx: usize,
    curr_addr: u64,
    kstart: u64,
    kend: u64,
    mbi_start: u64,
    mbi_end: u64,
}
```
During initialization (`Multiboot2FrameAllocator::init`):
1.  It iterates over the hardware memory areas provided by `boot_info.memory_map_tag()`.
2.  It ignores areas marked as `Reserved`, `ACPIReclaimable`, or `Defective`, searching only for areas marked `Available`.
3.  Once it finds the first available area, it sets its cursor (`curr_addr`) to that area's start address, rounded up to the nearest 4,096-byte boundary.

---

### Step 4: Allocating Frames on Demand
Located at lines 101–129 of [src/memory/allocator.rs](file:///home/aj/BeanieOS/src/memory/allocator.rs#L101-L129):

The allocator implements the `FrameAllocator<Size4KiB>` trait from the `x86_64` crate:
```rust
unsafe impl FrameAllocator<Size4KiB> for Multiboot2FrameAllocator<'_> {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        while self.area_idx < self.areas.len() {
            let area = &self.areas[self.area_idx];
            
            // Advance past non-available chunks
            if area.typ() != MemoryAreaType::Available {
                self.advance_area();
                continue;
            }

            // Bump the cursor through this area 4 KiB at a time
            while self.curr_addr + PAGE <= area.end_address() {
                let addr = self.curr_addr;
                self.curr_addr += PAGE; // Bump watermark

                // Check if this frame is safe
                if frame_is_reserved(addr, self.kstart, self.kend, self.mbi_start, self.mbi_end) {
                    continue; // Skip reserved frame
                }

                // Found a pristine, safe 4 KiB physical RAM frame!
                return Some(PhysFrame::containing_address(PhysAddr::new(addr)));
            }

            // Area exhausted, move to the next memory strip
            if !self.advance_area() {
                break;
            }
        }
        None // Out of physical memory!
    }
}
```

---

## 🎯 Summary Checklist
By the end of Phase 3, our operating system has:
1. Inspected the BIOS/UEFI physical memory map.
2. Filtered out hardware ROMs, the legacy 1 MiB zone, the kernel binary image, and the bootloader information structure.
3. Created a rock-solid Physical Memory Manager capable of handing out verified, pristine 4 KiB physical memory frames.
