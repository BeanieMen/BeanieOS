use super::super::consts::{
    LAPIC_SVR_ENABLE, SPURIOUS_VECTOR, lapic_eoi_reg, lapic_id_reg, lapic_svr_reg,
};

pub(super) unsafe fn init() {
    let svr = lapic_svr_reg() as *mut u32;
    let value = unsafe { core::ptr::read_volatile(svr) };
    unsafe {
        core::ptr::write_volatile(svr, value | LAPIC_SVR_ENABLE | SPURIOUS_VECTOR as u32);
    }
}

pub(super) unsafe fn id() -> u32 {
    unsafe { core::ptr::read_volatile(lapic_id_reg() as *const u32) >> 24 }
}

pub(super) unsafe fn send_eoi() {
    unsafe {
        core::ptr::write_volatile(lapic_eoi_reg() as *mut u32, 0);
    }
}
