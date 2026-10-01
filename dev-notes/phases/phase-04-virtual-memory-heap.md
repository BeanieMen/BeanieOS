# Phase 4: Virtual Memory Management (VMM) & Dynamic Heap Initialization

## 🌟 High-Level Overview
Have you ever wondered how data structures like `Vec`, `String`, `Box`, and `BTreeMap` work? They don't have a fixed size; they can grow and shrink dynamically while your program runs.

In low-level systems programming, you cannot create a `Vec` or allocate a `Box` out of thin air. The computer has no idea where to put them! Dynamic memory lives in an area called **The Heap**.

In this phase, we:
1. Construct a **Virtual Memory Mapper** that can alter the CPU's active page tables at runtime.
2. Carve out a special, dedicated virtual address range in the kernel (`0x4444_4444_0000`).
3. Take physical memory frames from our Phase 3 allocator and map them to those virtual addresses.
4. Hook up a **Heap Allocator** (`linked_list_allocator`), finally unlocking the standard Rust `alloc` crate!

---

## 📖 Layman's Glossary: Jargon Demystified

*   **Virtual Address vs. Physical Address:**
    *   **Physical Address:** The true, physical pin on the RAM chip (like GPS coordinates: Latitude 37.77, Longitude -122.41).
    *   **Virtual Address:** The address software sees (like a postal address: "742 Evergreen Terrace"). The CPU's internal hardware (the **MMU - Memory Management Unit**) translates the virtual address into the physical address on the fly.
*   **Page Table Mapping:**
    The process of writing an entry in the CPU's page tables saying: *"Whenever the software tries to access virtual address `0x4444_4444_0000`, silently redirect the electrical signals to physical RAM frame `0x100_0000`."*
*   **The Heap:**
    A pool of free memory managed by an allocator. When you do `Vec::new()` and push items, the allocator cuts off a small slice of the heap and gives it to the `Vec`.
*   **`linked_list_allocator`:**
    A simple heap algorithm that treats the heap like a linked list of free blocks. When you request 100 bytes, it finds a free block of at least 100 bytes, carves it out, and marks it as used. When you drop the variable, it stitches the block back into the free list.
*   **TLB (Translation Lookaside Buffer):**
    A high-speed cache inside the CPU that remembers recent virtual-to-physical address translations. Whenever we change a page table, we must "flush" the TLB (`invlpg` instruction), or the CPU will continue using old, stale translations.

---

## 🗺️ What Files are Involved?
1. [src/memory/allocator.rs](file:///home/aj/BeanieOS/src/memory/allocator.rs#L131-L150) — Reads `%cr3` to initialize `OffsetPageTable`.
2. [src/memory/heap.rs](file:///home/aj/BeanieOS/src/memory/heap.rs) — Maps the heap's virtual memory pages and initializes the `LockedHeap` global allocator.
3. [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L145-L147) — Orchestrates the mapping and heap initialization.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Connecting to the Active Level 4 Page Table
Located at lines 131–150 of [src/memory/allocator.rs](file:///home/aj/BeanieOS/src/memory/allocator.rs#L131-L150):

To create new memory mappings, our Rust code needs to be able to read and write the CPU's active page table.
```rust
fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    // 1. Ask the CPU for the physical address of the Level 4 table
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();

    // 2. Convert physical address to virtual address
    // Since early boot identity-mapped the first 8 GiB, offset is 0!
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}
```
*   `OffsetPageTable` is a helper from the `x86_64` crate. It allows us to say: *"Map virtual page X to physical frame Y"*, and it automatically handles creating any missing sub-tables (P3, P2, P1) in the middle!

---

### Step 2: Defining the Kernel Heap Territory
Located at lines 10–11 of [src/memory/heap.rs](file:///home/aj/BeanieOS/src/memory/heap.rs#L10-L11):

```rust
pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 1000 * 1024; // 1000 KiB (approx 1 Megabyte)
```
*   **Why `0x4444_4444_0000`?**
    In 64-bit mode, the virtual address space is unimaginably enormous ($2^{64}$ bytes). Kernel developers often pick distinctive, easily recognizable hexadecimal constants (like `0x4444_4444_0000`) for the heap. If a pointer ever starts with `0x4444`, you know immediately by glancing at a debugger that it is a heap allocation!

---

### Step 3: Mapping Heap Pages to Physical Frames
Located at lines 13–35 of [src/memory/heap.rs](file:///home/aj/BeanieOS/src/memory/heap.rs#L13-L35):

Before the heap allocator can use this memory, the CPU must be told that `0x4444_4444_0000` actually exists:
```rust
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    // 1. Calculate the range of 4 KiB virtual pages needed
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    // 2. Map every single page in that range
    for page in page_range {
        // Grab a real physical RAM frame from Phase 3's PMM
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
            
        // Mark the page as PRESENT in RAM and WRITABLE
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        
        // Write the translation into the page table and flush the TLB!
        unsafe { mapper.map_to(page, frame, flags, frame_allocator)?.flush() };
    }

    // 3. Hand the mapped memory region to the Linked List Allocator
    unsafe { ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE) };
    Ok(())
}
```

---

### Step 4: The Global Allocator Declaration
Located at lines 37–38 of [src/memory/heap.rs](file:///home/aj/BeanieOS/src/memory/heap.rs#L37-L38):

```rust
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
```
The `#[global_allocator]` attribute tells the Rust compiler: *"Whenever any Rust code in this kernel uses `alloc::vec::Vec`, `alloc::boxed::Box`, or `alloc::string::String`, route their allocations through this `ALLOCATOR` instance."*

---

## 🎯 Summary Checklist
By the end of Phase 4, our operating system has:
1. Created an `OffsetPageTable` manager to dynamically manipulate page tables.
2. Mapped 1000 KiB of virtual memory starting at `0x4444_4444_0000` to physical RAM frames.
3. Flushed the CPU's TLB cache for every newly mapped page.
4. Initialized the `LockedHeap` global allocator, unlocking the full power of dynamic Rust collections!
