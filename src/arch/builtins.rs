use core::ffi::c_void;

#[inline(always)]
fn assert_readable(ptr: *const u8) {
    assert!(!ptr.is_null(), "null source pointer");
}

#[inline(always)]
fn assert_writable(ptr: *mut u8) {
    assert!(!ptr.is_null(), "null destination pointer");
}

#[inline(always)]
unsafe fn rep_movsb(dest: &mut *mut u8, src: &mut *const u8, count: usize) {
    unsafe {
        core::arch::asm!(
            "cld",
            "rep movsb",
            inout("rdi") *dest,
            inout("rsi") *src,
            in("rcx") count,
            options(nostack),
        );
    }
}

#[inline(always)]
unsafe fn rep_movsb_backwards(dest: &mut *mut u8, src: &mut *const u8, count: usize) {
    *dest = dest.wrapping_add(count);
    *src = src.wrapping_add(count);

    unsafe {
        core::arch::asm!(
            "std",
            "rep movsb",
            "cld",
            inout("rdi") *dest,
            inout("rsi") *src,
            in("rcx") count,
            options(nostack),
        );
    }
}

#[inline(always)]
unsafe fn rep_stosb(dest: &mut *mut u8, value: u8, count: usize) {
    unsafe {
        core::arch::asm!(
            "cld",
            "rep stosb",
            inout("rdi") *dest,
            in("rax") value as u64 as u32,
            in("rcx") count,
            options(nostack),
        );
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut c_void, value: i32, count: usize) -> *mut c_void {
    let mut bytes = dest as *mut u8;

    assert_writable(bytes);

    if count != 0 {
        unsafe { rep_stosb(&mut bytes, value as u8, count) };
    }

    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(
    dest: *mut c_void,
    src: *const c_void,
    count: usize,
) -> *mut c_void {
    let mut d = dest as *mut u8;
    let mut s = src as *const u8;

    assert_writable(d);
    assert_readable(s);

    if count != 0 && !core::ptr::eq(d, s) {
        unsafe { rep_movsb(&mut d, &mut s, count) };
    }

    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(
    dest: *mut c_void,
    src: *const c_void,
    count: usize,
) -> *mut c_void {
    let mut d = dest as *mut u8;
    let mut s = src as *const u8;

    assert_writable(d);
    assert_readable(s);

    if count == 0 || core::ptr::eq(d, s) {
        return dest;
    }

    if (d as usize) < (s as usize) {
        unsafe { rep_movsb(&mut d, &mut s, count) };
    } else {
        unsafe { rep_movsb_backwards(&mut d, &mut s, count) };
    }

    dest
}

// SAFETY: caller guarantees both ranges readable for `count` bytes. Signed
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, count: usize) -> i32 {
    let (a, b) = unsafe {
        (
            core::slice::from_raw_parts(a as *const u8, count),
            core::slice::from_raw_parts(b as *const u8, count),
        )
    };

    match a.iter().zip(b).position(|(x, y)| x != y) {
        None => 0,
        Some(i) => a[i] as i32 - b[i] as i32,
    }
}
