# Phase 8: ACPI Discovery, Legacy 8259 PIC Disabling, & the APIC Subsystem

## 🌟 High-Level Overview
Your computer contains thousands of electrical connections between devices (keyboards, hard drives, timers) and the CPU. How does the CPU know which wire belongs to which device?

In the 1980s, the IBM PC used two chips called the **8259 PIC (Programmable Interrupt Controller)**. But the 8259 PIC is a dinosaur: it only supports 15 devices, does not support multi-core processors, and clashes with modern 64-bit hardware.

Modern systems use the **APIC (Advanced Programmable Interrupt Controller)** architecture:
1.  **Local APIC (LAPIC):** One exists inside *every single CPU core*. It manages timers and delivers interrupts directly to its core.
2.  **I/O APIC:** Sits on the motherboard chipset, intercepts device signals, and routes them to the appropriate CPU core.

In this phase, we:
1. Walk the motherboard's **ACPI** tables to find the **MADT (Multiple APIC Description Table)**.
2. Discover where the LAPIC and I/O APIC live in physical memory.
3. Silence the ancient 8259 PIC forever.
4. Program the Local APIC and start its periodic timer.
5. Program the I/O APIC to route the PS/2 keyboard to our CPU core.
6. Finally run `sti` (Set Interrupt Flag) to start receiving live hardware interrupts!

---

## 📖 Layman's Glossary: Jargon Demystified

*   **ACPI (Advanced Configuration and Power Interface):**
    A library of hardware description tables burned into the motherboard's BIOS/UEFI ROM. It tells the operating system what hardware exists without the OS having to guess.
*   **RSDP (Root System Description Pointer):**
    A 36-byte signpost in memory pointing to the master table of all ACPI tables (the **XSDT** or **RSDT**).
*   **MADT (Multiple APIC Description Table):**
    The specific ACPI table that contains the hardware addresses of the Local APIC, the I/O APIC, and interrupt override rules.
*   **GSI (Global System Interrupt):**
    A unified numbering system for every interrupt pin on the motherboard.
*   **Interrupt Source Override:**
    A motherboard quirk. On ancient PCs, the PS/2 keyboard was ISA IRQ 1. On some motherboards, IRQ 1 is wired to GSI 1; on others, it is remapped to GSI 2. The MADT tells us the truth!
*   **MMIO (Memory-Mapped I/O):**
    Instead of using special assembly instructions to talk to a chip, the chip's internal control registers are mapped to pretend memory addresses. Reading or writing that memory address sends electrical commands straight into the chip.
*   **EOI (End of Interrupt):**
    When the CPU finishes handling an interrupt, it *must* write `0` to the LAPIC's EOI register. If you forget to send an EOI, the LAPIC assumes you are still busy and refuses to deliver any more interrupts forever!
*   **`sti` (Set Interrupt Flag):**
    The master assembly instruction that un-mutes the CPU. Interrupts are now allowed to fire.

---

## 🗺️ What Files are Involved?
1. [src/arch/interrupts/pic/mod.rs](file:///home/aj/BeanieOS/src/arch/interrupts/pic/mod.rs) — Master controller for disabling the 8259 PIC and bringing up APIC.
2. [src/arch/interrupts/pic/madt.rs](file:///home/aj/BeanieOS/src/arch/interrupts/pic/madt.rs) — ACPI parser searching for table `"APIC"`.
3. [src/arch/interrupts/pic/lapic.rs](file:///home/aj/BeanieOS/src/arch/interrupts/pic/lapic.rs) — Local APIC driver and periodic timer programmer.
4. [src/arch/interrupts/pic/ioapic.rs](file:///home/aj/BeanieOS/src/arch/interrupts/pic/ioapic.rs) — Motherboard I/O APIC router configuration.

---

## 🪜 Step-by-Step Code Walkthrough

### Step 1: Finding and Parsing the MADT
Located at lines 31–117 of [src/arch/interrupts/pic/madt.rs](file:///home/aj/BeanieOS/src/arch/interrupts/pic/madt.rs#L31-L117):

Our kernel receives the root ACPI pointer from Multiboot2, loops through the table pointers, and searches for the signature ASCII string `"APIC"`:
```rust
unsafe fn find_madt(root: usize) -> usize {
    // ... loops through XSDT / RSDT entries ...
    let signature = core::slice::from_raw_parts(table as *const u8, 4);
    if signature == b"APIC" {
        return table; // Found the MADT!
    }
}
```
Inside the MADT, it parses the variable-length records:
1.  **Offset +36:** The 32-bit physical address of the Local APIC (typically `0xFEE0_0000`).
2.  **Type 1 Record:** The I/O APIC address (typically `0xFEC0_0000`) and its `gsi_base`.
3.  **Type 2 Record (Override):** Checks if legacy IRQ 1 (Keyboard) is remapped to a different GSI pin.

---

### Step 2: Silencing the Legacy 8259 PIC
Located at lines 7–18 of [src/arch/interrupts/pic/mod.rs](file:///home/aj/BeanieOS/src/arch/interrupts/pic/mod.rs#L7-L18):

```rust
fn disable_legacy_pic() {
    use x86_64::instructions::port::Port;
    unsafe {
        // Port 0x21 is Master PIC Mask; Port 0xA1 is Slave PIC Mask
        Port::new(0x21).write(0xFFu8);
        Port::new(0xA1).write(0xFFu8);
    }
}
```
*   Writing `0xFF` (all bits set) masks every single interrupt line on the old 8259 chips. They will never speak again.

---

### Step 3: Enabling the Local APIC & Configuring the Timer
Located at lines 6–24 of [src/arch/interrupts/pic/lapic.rs](file:///home/aj/BeanieOS/src/arch/interrupts/pic/lapic.rs#L6-L24):

```rust
pub(super) unsafe fn init() {
    let svr = lapic_svr_reg() as *mut u32; // Offset 0xF0
    let value = unsafe { core::ptr::read_volatile(svr) };
    // Bit 8 = Software Enable | Vector 0xFF for Spurious Interrupts
    core::ptr::write_volatile(svr, value | LAPIC_SVR_ENABLE | SPURIOUS_VECTOR as u32);
}

pub(super) unsafe fn init_timer(initial_count: u32) {
    // 1. Divide bus clock by 16
    core::ptr::write_volatile(lapic_timer_divide_reg() as *mut u32, LAPIC_TIMER_DIVIDE_16);

    // 2. Set to Periodic Mode on Vector 32
    core::ptr::write_volatile(
        lapic_timer_reg() as *mut u32,
        LAPIC_TIMER_PERIODIC | LAPIC_TIMER_VECTOR as u32,
    );

    // 3. Set the countdown number (starts ticking immediately!)
    core::ptr::write_volatile(lapic_timer_initial_reg() as *mut u32, initial_count);
}
```
*   The timer counts down from `initial_count` (`0x100_000`). Whenever it hits 0, it triggers **Interrupt Vector 32** on our CPU and automatically resets back to `0x100_000`. This gives our OS a continuous heartbeat!

---

### Step 4: Routing the Keyboard through the I/O APIC
Located at lines 12–38 of [src/arch/interrupts/pic/ioapic.rs](file:///home/aj/BeanieOS/src/arch/interrupts/pic/ioapic.rs#L12-L38):

The I/O APIC has 24+ "Redirection Table Entries". Each entry is a 64-bit register split into a low and high 32-bit register:
*   **High Register (bits 56–63):** Target CPU Core's LAPIC ID.
*   **Low Register (bits 0–7):** The IDT Vector number to send to that CPU core.

```rust
pub(super) unsafe fn init() {
    let parsed = madt::get();
    let index = parsed.keyboard_gsi - parsed.ioapic_gsi_base;
    let low = 0x10 + index * 2;
    let high = low + 1;

    let lapic_id = unsafe { lapic::id() };

    // Direct keyboard interrupts to our core's LAPIC ID
    ioapic_write(parsed.ioapic_address, high as u8, lapic_id << 24);
    
    // Deliver as IDT Vector 33 (KEYBOARD_VECTOR)
    ioapic_write(parsed.ioapic_address, low as u8, KEYBOARD_VECTOR as u32);
}
```

---

### Step 5: Turning On the Firehose (`sti`)
Located at line 159 of [src/main.rs](file:///home/aj/BeanieOS/src/main.rs#L159):

```rust
x86_64::instructions::interrupts::enable(); // sti
```
The CPU executes the `sti` instruction. The interrupt shield drops. Immediately, the Local APIC begins sending timer ticks, and keyboard strokes trigger real-time interrupt handlers!

---

## 🎯 Summary Checklist
By the end of Phase 8, our operating system has:
1. Located and parsed the ACPI MADT table.
2. Disabled the legacy 8259 dual PIC chips.
3. Activated the Local APIC and programmed a periodic hardware timer heartbeat.
4. Programmed the I/O APIC to route keyboard interrupts to Vector 33.
5. Enabled CPU interrupts globally (`sti`).
