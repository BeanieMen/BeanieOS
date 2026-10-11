use alloc::string::String;
use alloc::vec::Vec;

use crate::{
    kdebug, kerror,
    memory::mmu::{USER_END, USER_START},
    storage::vfs::file::OpenFlags,
    task::{identity::current_pid, process, scheduler},
};
use x86_64::VirtAddr;

use super::entry;
use super::error::{Errno, SyscallResult};
use super::numbers::*;

pub(crate) fn dispatch(number: u64, args: [u64; 6]) -> SyscallResult {
    let Ok(syscall) = Syscall::try_from(number) else {
        return Err(Errno::NoSys);
    };

    match syscall {
        Syscall::Read => sys_read(args[0], args[1], args[2]),
        Syscall::Write => sys_write(args[0], args[1], args[2]),
        Syscall::Open => sys_open(args[0], args[1]),
        Syscall::Close => sys_close(args[0]),
        Syscall::Lseek => sys_lseek(args[0], args[1] as i64, args[2] as u32),
        Syscall::Exit => sys_exit(args[0]),
        Syscall::ArchPrctl => sys_arch_prctl(args[0], args[1]),
        Syscall::Getpid => sys_getpid(),
        Syscall::SchedYield => sys_yield(),
        Syscall::Getdents64 => sys_getdents(args[0], args[1], args[2]),
        Syscall::OpenDir => sys_open_dir(args[0]),
        Syscall::Mkdir => sys_mkdir(args[0]),
        Syscall::Create => sys_create(args[0]),
        Syscall::Execve => sys_execve(args[0], args[1], args[2]),
        Syscall::Unlink => sys_unlink(args[0]),
        Syscall::Rmdir => sys_rmdir(args[0]),
    }
}
const MAX_ARGS: usize = 64;
const MAX_ARG_LEN: usize = 4096;

fn read_string_array(array: u64, limit: usize) -> Result<Vec<Vec<u8>>, ()> {
    let mut out = Vec::new();
    for index in 0..limit {
        let slot = array.checked_add((index * 8) as u64).ok_or(())?;

        let mut word = [0u8; 8];
        entry::copy_from_user(&mut word, slot)?;
        let ptr = u64::from_le_bytes(word);
        if ptr == 0 {
            return Ok(out);
        }

        out.push(entry::user_path(ptr)?);
    }
    Err(())
}

fn sys_execve(path: u64, argv: u64, envp: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };
    let Ok(argv) = read_string_array(argv, MAX_ARGS) else {
        kerror!("execve: argv is not a terminated array of user pointers");
        return Err(Errno::Fault);
    };
    let Ok(envp) = read_string_array(envp, MAX_ARGS) else {
        kerror!("execve: envp is not a terminated array of user pointers");
        return Err(Errno::Fault);
    };

    match crate::task::exec::execve(&path, &argv, &envp) {
        Ok(never) => never,
        Err(why) => {
            kerror!("execve({}): {why}", String::from_utf8_lossy(&path));
            Err(Errno::Invalid)
        }
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

fn sys_lseek(fd: u64, offset: i64, whence: u32) -> SyscallResult {
    let Some(result) = process::with_current_fds(|fds| fds.lseek(fd, offset, whence)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(pos) => Ok(pos),

        Err(why) => match why {
            "bad file descriptor" => Err(Errno::BadFd),
            "seek overflow" => Err(Errno::Overflow),
            "seek before start of file" => Err(Errno::Invalid),
            _ => {
                kdebug!("lseek({fd}, {offset}, {whence}): {why}");
                Err(Errno::Invalid)
            }
        },
    }
}

fn sys_open_dir(path: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };

    let Some(result) = process::with_current_fds(|fds| fds.open_dir(&path)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(fd) => Ok(fd),
        Err(_) => Err(Errno::NoEntry),
    }
}

fn sys_arch_prctl(code: u64, addr: u64) -> SyscallResult {
    const ARCH_SET_FS: u64 = 0x1002;
    const ARCH_SET_GS: u64 = 0x1001;

    if code != ARCH_SET_FS {
        return Err(Errno::Invalid);
    }
    if addr < USER_START || addr >= USER_END {
        return Err(Errno::Fault);
    }
    unsafe {
        x86_64::registers::model_specific::FsBase::write(VirtAddr::new(addr));
    }

    Ok(0)
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

    let Some(result) = process::with_current_fds(|fds| fds.getdents(fd, &mut staging[..capped]))
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
