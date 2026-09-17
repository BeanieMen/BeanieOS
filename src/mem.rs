use core::ffi::c_void;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut c_void, value: i32, count: usize) -> *mut c_void {
    let bytes = dest as *mut u8;
    for i in 0..count {
        unsafe {
            bytes.add(i).write(value as u8);
        }
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, count: usize) -> *mut c_void {
    let d = dest as *mut u8;
    let s = src as *const u8;
    for i in 0..count {
        unsafe {
            d.add(i).write(s.add(i).read());
        }
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, count: usize) -> i32 {
    let a_bytes = a as *const u8;
    let b_bytes = b as *const u8;
    for i in 0..count {
        unsafe {
            let ai = a_bytes.add(i).read();
            let bi = b_bytes.add(i).read();
            if ai != bi {
                return ai as i32 - bi as i32;
            }
        }
    }
    0
}