use super::super::consts::{
    LAPIC_EOI_REG, LAPIC_ID_REG, LAPIC_SVR, LAPIC_SVR_ENABLE, SPURIOUS_VECTOR,
};

pub(super) unsafe fn init() {
    let svr = LAPIC_SVR as *mut u32;
    let value = unsafe { core::ptr::read_volatile(svr) };
    unsafe {
        core::ptr::write_volatile(svr, value | LAPIC_SVR_ENABLE | SPURIOUS_VECTOR as u32);
    }
}

pub(super) unsafe fn id() -> u32 {
    unsafe { core::ptr::read_volatile(LAPIC_ID_REG as *const u32) >> 24 }
}

pub(super) unsafe fn send_eoi() {
    unsafe {
        core::ptr::write_volatile(LAPIC_EOI_REG as *mut u32, 0);
    }
}
