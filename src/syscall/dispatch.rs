use crate::{
    storage::vfs::file::OpenFlags,
    task::{identity::current_pid, process, scheduler},
};

use super::error::{Errno, encode_result};
use super::numbers::*;

pub fn dispatch(number: u64, args: [u64; 6]) -> u64 {
    let Ok(syscall) = Syscall::try_from(number) else {
        return encode_result(Err(Errno::NoSys));
    };

    match syscall {
        Syscall::Exit => sys_exit(args[0]),
        Syscall::Write => sys_write(args[0], args[1], args[2]),
        Syscall::Read => sys_read(args[0], args[1], args[2]),
        Syscall::Open => sys_open(args[0], args[1]),
        Syscall::Close => sys_close(args[0]),
        Syscall::Mkdir => sys_mkdir(args[0]),
        Syscall::Getdents => sys_getdents(args[0], args[1], args[2]),
        Syscall::Yield => sys_yield(),
        Syscall::Getpid => sys_getpid(),
        Syscall::Unlink => sys_unlink(args[0]),
        Syscall::Rmdir => sys_rmdir(args[0]),
    }
}

fn sys_exit(code: u64) -> ! {
    let pid = current_pid();

    process::exit(pid, code as i32);
    scheduler::exit();
}

fn sys_write(fd: u64, buf: u64, len: u64) -> u64 {
    let Some(result) = process::with_current_fds(|fds| {
        fds.write(fd, unsafe {
            core::slice::from_raw_parts(buf as *const u8, len as usize)
        })
    }) else {
        return encode_result(Err(Errno::BadFd));
    };

    match result {
        Ok(written) => written as u64,
        Err(_) => encode_result(Err(Errno::Invalid)),
    }
}

fn sys_read(fd: u64, buf: u64, len: u64) -> u64 {
    let Some(result) = process::with_current_fds(|fds| {
        fds.read(fd, unsafe {
            core::slice::from_raw_parts_mut(buf as *mut u8, len as usize)
        })
    }) else {
        return encode_result(Err(Errno::BadFd));
    };

    match result {
        Ok(read) => read as u64,
        Err(_) => encode_result(Err(Errno::Invalid)),
    }
}

fn sys_open(path: u64, flags: u64) -> u64 {
    let path = match process::str_from_ptr(path) {
        Ok(path) => path,
        Err(_) => return encode_result(Err(Errno::Fault)),
    };

    let open_flags = match flags {
        0 => OpenFlags::READ,
        1 => OpenFlags::WRITE,
        2 => OpenFlags::RDWR,
        _ => return encode_result(Err(Errno::Invalid)),
    };

    let Some(result) = process::with_current_fds(|fds| fds.open(path.as_bytes(), open_flags))
    else {
        return encode_result(Err(Errno::BadFd));
    };

    match result {
        Ok(fd) => fd,
        Err(_) => return encode_result(Err(Errno::NoEntry)),
    }
}

fn sys_close(fd: u64) -> u64 {
    let Some(result) = process::with_current_fds(|fds| fds.close(fd)) else {
        return encode_result(Err(Errno::BadFd));
    };

    match result {
        Ok(()) => 0,
        Err(_) => encode_result(Err(Errno::BadFd)),
    }
}

fn sys_mkdir(_path: u64) -> u64 {
    todo!()
}

fn sys_getdents(_fd: u64, _buf: u64, _len: u64) -> u64 {
    todo!()
}

fn sys_yield() -> u64 {
    scheduler::yield_now();
    0
}

fn sys_getpid() -> u64 {
    current_pid().as_u64()
}

fn sys_unlink(_path: u64) -> u64 {
    todo!()
}

fn sys_rmdir(_path: u64) -> u64 {
    todo!()
}
