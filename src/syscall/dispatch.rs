use crate::{
    storage::vfs::file::OpenFlags,
    task::{identity::current_pid, process, scheduler},
};

use super::entry;
use super::error::{Errno, SyscallResult};
use super::numbers::*;

pub(crate) fn dispatch(number: u64, args: [u64; 6]) -> SyscallResult {
    let Ok(syscall) = Syscall::try_from(number) else {
        return Err(Errno::NoSys);
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

fn sys_write(fd: u64, buf: u64, len: u64) -> SyscallResult {
    if len > 4096 {
        return Err(Errno::Invalid);
    }

    let mut staging = [0u8; 4096];
    let capped = len as usize;

    if entry::copy_from_user(&mut staging[..capped], buf).is_err() {
        return Err(Errno::Fault);
    }

    let Some(result) = process::with_current_fds(|fds| fds.write(fd, &staging[..capped])) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(written) => Ok(written as u64),
        Err(_) => Err(Errno::Invalid),
    }
}

fn sys_read(fd: u64, buf: u64, len: u64) -> SyscallResult {
    if len > 4096 {
        return Err(Errno::Invalid);
    }

    let mut staging = [0u8; 4096];

    let Some(result) = process::with_current_fds(|fds| fds.read(fd, &mut staging[..len as usize]))
    else {
        return Err(Errno::BadFd);
    };

    let read = match result {
        Ok(read) => read,
        Err(_) => return Err(Errno::Invalid),
    };

    if entry::copy_to_user(buf, &staging[..read]).is_err() {
        return Err(Errno::Fault);
    }

    Ok(read as u64)
}

fn sys_open(path: u64, flags: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };

    let open_flags = match flags {
        0 => OpenFlags::READ,
        1 => OpenFlags::WRITE,
        2 => OpenFlags::RDWR,
        _ => return Err(Errno::Invalid),
    };

    let Some(result) = process::with_current_fds(|fds| fds.open(&path, open_flags)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(fd) => Ok(fd),
        Err(_) => return Err(Errno::NoEntry),
    }
}

fn sys_close(fd: u64) -> SyscallResult {
    let Some(result) = process::with_current_fds(|fds| fds.close(fd)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(()) => Ok(0),
        Err(_) => Err(Errno::BadFd),
    }
}

fn sys_mkdir(_path: u64) -> SyscallResult {
    todo!()
}

fn sys_getdents(_fd: u64, _buf: u64, _len: u64) -> SyscallResult {
    todo!()
}

fn sys_yield() -> SyscallResult {
    scheduler::yield_now();
    Ok(0)
}

fn sys_getpid() -> SyscallResult {
    Ok(current_pid().as_u64())
}

fn sys_unlink(_path: u64) -> SyscallResult {
    todo!()
}

fn sys_rmdir(_path: u64) -> SyscallResult {
    todo!()
}
