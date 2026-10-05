use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use x86_64::instructions::interrupts;
use x86_64::registers::control::Cr0;

use super::regs::*;
use crate::arch::interrupts::consts::TICK_MS;
use crate::arch::interrupts::pic;
use crate::arch::interrupts::vectors::ticks;
use crate::kinfo;

// Deadline for one command; the timer wakes the halt below.
const TIMEOUT_MS: u64 = 500;

const CR0_INTERRUPT_ENABLE: u64 = 1;

static ABAR: AtomicUsize = AtomicUsize::new(0);
static PORTS: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicBool = AtomicBool::new(false);

// How many times the handler ran, so a timeout can tell "the interrupt never
// came" from "it came and meant nothing".
static HANDLED: AtomicUsize = AtomicUsize::new(0);

// Routes the adapter's line. Must run before any port unmasks PxIE.
pub(crate) fn attach(abar: usize, ports: u32, pin: u8, line: u8) {
    ABAR.store(abar, Ordering::Relaxed);
    PORTS.store(ports as usize, Ordering::Relaxed);

    // Config 0x3C holds the GSI. Not the pin: pin 1 is INTA#, the keyboard's line.
    let gsi = line as u32;

    kinfo!("  interrupt: pin {pin} -> gsi {gsi} (line register {line})");

    pic::ioapic::route(gsi, crate::arch::interrupts::consts::AHCI_VECTOR);
}

pub extern "x86-interrupt" fn on_interrupt(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    HANDLED.fetch_add(1, Ordering::Relaxed);

    let abar = ABAR.load(Ordering::Relaxed);
    let ports = PORTS.load(Ordering::Relaxed);

    if abar != 0 {
        for port in 0..32 {
            if ports & (1 << port) == 0 {
                continue;
            }

            let base = abar + PORT_BASE + port * PORT_STRIDE;
            let status = reg(base, PX_IS);

            if status & COMMAND_BITS != 0 {
                // Write-1-to-clear; latched bits re-assert and storm.
                set_reg(base, PX_IS, status & COMMAND_BITS);
                DONE.store(true, Ordering::Release);
            }
        }
    }

    unsafe {
        pic::eoi();
    }
}

pub(crate) fn wait(base: usize) -> Result<(), &'static str> {
    let was_enabled = Cr0::read().bits() & CR0_INTERRUPT_ENABLE != 0;
    let deadline = ticks() + TIMEOUT_MS.div_ceil(TICK_MS);

    if !was_enabled {
        interrupts::enable();
    }

    while !DONE.load(Ordering::Acquire) {
        if ticks() >= deadline {
            let is = reg(base, PX_IS);
            let ci = reg(base, PX_CI);
            let ie = reg(base, PX_IE);
            let task = reg(base, PX_TFD);

            // Free the slot and mask, so a late completion raises nothing.
            set_reg(base, PX_IE, 0);
            set_reg(base, PX_CI, 0);

            if !was_enabled {
                interrupts::disable();
            }

            kinfo!(
                "    timeout after {TIMEOUT_MS} ms (ticks now {}): PxIS={is:#x} PxCI={ci:#x} PxIE={ie:#x} PxTFD={task:#x} handled {}",
                ticks(),
                HANDLED.load(Ordering::Relaxed)
            );

            return Err("command timed out");
        }

        // `sti; hlt` as one, or an interrupt between them sleeps a tick.
        interrupts::enable_and_hlt();
    }

    if !was_enabled {
        interrupts::disable();
    }

    set_reg(base, PX_IE, 0);

    Ok(())
}

pub(crate) fn arm(base: usize) {
    DONE.store(false, Ordering::Relaxed);

    // Bits latched by an earlier command would fire before this one starts.
    set_reg(base, PX_IS, u32::MAX);
    set_reg(base, PX_IE, COMMAND_BITS);
}
