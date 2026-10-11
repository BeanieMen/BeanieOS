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
        Syscall::Create => sys_create(args[0]),
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

    if fd == 1 || fd == 2 {
        for &byte in &staging[..capped] {
            crate::print!("{}", byte as char);
        }
        return Ok(capped as u64);
    }

    let Some(result) = process::with_current_fds(|fds| fds.write(fd, &staging[..capped])) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(written) => Ok(written as u64),
        Err(_) => Err(Errno::Invalid),
    }
}


/// Parks the caller until the keyboard wakes it.
///
/// The thread goes on the wait list and off the run queue, so the scheduler
/// runs someone else. Nothing is switched from the interrupt handler: the IRQ
/// only makes the thread runnable, and the switch happens here, in ordinary
/// kernel code on this thread's own stack.
fn wait_for_key() {
    let id = crate::task::identity::current_thread_id();

    crate::arch::input::add_waiter(id);

    let switched = scheduler::block_current();

    crate::arch::input::remove_waiter(id);

    if !switched {
        x86_64::instructions::hlt();
    }
}

fn sys_read(fd: u64, buf: u64, len: u64) -> SyscallResult {
    if len > 4096 {
        return Err(Errno::Invalid);
    }

    if fd == 0 {
        let mut staging = [0u8; 4096];
        let mut read = 0usize;
        let target_len = (len as usize).min(staging.len());

        while read < target_len {
            if let Some(ch) = crate::arch::input::pop_key() {
                staging[read] = ch as u8;
                read += 1;
                if ch == '\n' || ch == '\r' {
                    break;
                }

                continue;
            }

            wait_for_key();
        }

        if read > 0 && entry::copy_to_user(buf, &staging[..read]).is_err() {
            return Err(Errno::Fault);
        }

        return Ok(read as u64);
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

    let read_dir = flags == 3;

    let open_flags = match flags {
        0 => OpenFlags::READ,
        1 => OpenFlags::WRITE,
        2 => OpenFlags::RDWR,
        3 => OpenFlags::READ_DIR,
        _ => return Err(Errno::Invalid),
    };

    let Some(result) = process::with_current_fds(|fds| {
        if read_dir {
            fds.open_dir(&path)
        } else {
            fds.open(&path, open_flags)
        }
    }) else {
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

fn sys_mkdir(path: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };

    let Some(result) = process::with_current_fds(|fds| fds.mkdir(&path)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(()) => Ok(0),
        Err(_) => Err(Errno::NoEntry),
    }
}

fn sys_getdents(fd: u64, buf: u64, len: u64) -> SyscallResult {
    if len > 4096 {
        return Err(Errno::Invalid);
    }

    let mut staging = [0u8; 4096];
    let capped = len as usize;

    let Some(result) =
        process::with_current_fds(|fds| fds.getdents(fd, &mut staging[..capped]))
    else {
        return Err(Errno::BadFd);
    };

    let read = match result {
        Ok(read) => read,
        Err(_) => return Err(Errno::Invalid),
    };

    if read == 0 {
        return Ok(0);
    }

    if entry::copy_to_user(buf, &staging[..read]).is_err() {
        return Err(Errno::Fault);
    }

    Ok(read as u64)
}

fn sys_yield() -> SyscallResult {
    scheduler::yield_now();

    Ok(0)
}

fn sys_getpid() -> SyscallResult {
    Ok(current_pid().as_u64())
}

fn sys_unlink(path: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };

    let Some(result) = process::with_current_fds(|fds| fds.unlink(&path)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(()) => Ok(0),
        Err(_) => Err(Errno::NoEntry),
    }
}

fn sys_rmdir(path: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };

    let Some(result) = process::with_current_fds(|fds| fds.rmdir(&path)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(()) => Ok(0),
        Err(_) => Err(Errno::NoEntry),
    }
}

/// `touch`, expressed as a create that tolerates an existing file.
fn sys_create(path: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };

    let Some(result) = process::with_current_fds(|fds| fds.touch(&path)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(()) => Ok(0),
        Err(_) => Err(Errno::NoEntry),
    }
}
