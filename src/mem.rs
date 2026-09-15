#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(
    dest: *mut u8,
    value: i32,
    count: usize,
) -> *mut u8 {
    for i in 0..count {
        unsafe {
            *dest.add(i) = value as u8;
        }
    }

    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(
    dest: *mut u8,
    src: *const u8,
    count: usize,
) -> *mut u8 {
    for i in 0..count {
        unsafe {
            *dest.add(i) = *src.add(i);
        }
    }

    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(
    a: *const u8,
    b: *const u8,
    count: usize,
) -> i32 {
    for i in 0..count {
        let a = unsafe { *a.add(i) };
        let b = unsafe { *b.add(i) };

        if a != b {
            return i32::from(a) - i32::from(b);
        }
    }

    0
}