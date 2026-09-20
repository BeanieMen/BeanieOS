use x86_64::structures::idt::{InterruptDescriptorTable, PageFaultErrorCode};

use crate::arch::gdt;

// Diagnostic handlers for every CPU fault vector. Whichever fault fires
// first prints its own name instead of cascading into a double fault, so a
// `DOUBLE FAULT` message with no preceding fault line means the failure is
// in exception delivery itself (IDT/stack), not in a handler.
macro_rules! fault_handler {
    ($name:ident, $label:literal) => {
        extern "x86-interrupt" fn $name(stack_frame: x86_64::structures::idt::InterruptStackFrame) {
            crate::println!(concat!("EXCEPTION: ", $label, "\n{:#?}"), stack_frame);
            loop {
                x86_64::instructions::hlt();
            }
        }
    };
}

macro_rules! fault_handler_with_err {
    ($name:ident, $label:literal) => {
        extern "x86-interrupt" fn $name(
            stack_frame: x86_64::structures::idt::InterruptStackFrame,
            error_code: u64,
        ) {
            crate::println!(
                concat!("EXCEPTION: ", $label, "\nError Code: {:#x}\n{:#?}"),
                error_code,
                stack_frame
            );
            loop {
                x86_64::instructions::hlt();
            }
        }
    };
}

fault_handler!(divide_error_handler, "DIVIDE ERROR");
fault_handler!(debug_handler, "DEBUG EXCEPTION");
fault_handler!(nmi_handler, "NON-MASKABLE INTERRUPT");
fault_handler!(overflow_handler, "OVERFLOW");
fault_handler!(bound_range_handler, "BOUND RANGE EXCEEDED");
fault_handler!(invalid_opcode_handler, "INVALID OPCODE");
fault_handler!(device_not_available_handler, "DEVICE NOT AVAILABLE");
fault_handler!(simd_handler, "SIMD FLOATING-POINT EXCEPTION");
fault_handler!(virtualization_handler, "VIRTUALIZATION EXCEPTION");

fault_handler_with_err!(invalid_tss_handler, "INVALID TSS");
fault_handler_with_err!(segment_not_present_handler, "SEGMENT NOT PRESENT");
fault_handler_with_err!(stack_segment_fault_handler, "STACK SEGMENT FAULT");
fault_handler_with_err!(general_protection_handler, "GENERAL PROTECTION FAULT");
fault_handler_with_err!(alignment_check_handler, "ALIGNMENT CHECK");
fault_handler_with_err!(cp_protection_handler, "CONTROL PROTECTION EXCEPTION");

extern "x86-interrupt" fn machine_check_handler(
    stack_frame: x86_64::structures::idt::InterruptStackFrame,
) -> ! {
    crate::println!("EXCEPTION: MACHINE CHECK\n{:#?}", stack_frame);
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn breakpoint_handler(
    stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    crate::println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: x86_64::structures::idt::InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: x86_64::structures::idt::InterruptStackFrame,
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

pub(crate) fn register_faults(idt: &mut InterruptDescriptorTable) {
    idt.divide_error.set_handler_fn(divide_error_handler);
    idt.debug.set_handler_fn(debug_handler);
    idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.overflow.set_handler_fn(overflow_handler);
    idt.bound_range_exceeded.set_handler_fn(bound_range_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.device_not_available
        .set_handler_fn(device_not_available_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt.invalid_tss.set_handler_fn(invalid_tss_handler);
    idt.segment_not_present
        .set_handler_fn(segment_not_present_handler);
    idt.stack_segment_fault
        .set_handler_fn(stack_segment_fault_handler);
    idt.general_protection_fault
        .set_handler_fn(general_protection_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.alignment_check.set_handler_fn(alignment_check_handler);
    idt.machine_check.set_handler_fn(machine_check_handler);
    idt.simd_floating_point.set_handler_fn(simd_handler);
    idt.virtualization.set_handler_fn(virtualization_handler);
    idt.cp_protection_exception
        .set_handler_fn(cp_protection_handler);
}
