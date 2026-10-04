use crate::task::{identity::current_pid, process, scheduler};

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
    todo!()
}

fn sys_read(fd: u64, buf: u64, len: u64) -> u64 {
    todo!()
}

fn sys_open(path: u64, flags: u64) -> u64 {
    todo!()
}

fn sys_close(fd: u64) -> u64 {
    todo!()
}

fn sys_mkdir(path: u64) -> u64 {
    todo!()
}

fn sys_getdents(fd: u64, buf: u64, len: u64) -> u64 {
    todo!()
}

fn sys_yield() -> u64 {
    scheduler::yield_now();
    0
}

fn sys_getpid() -> u64 {
    current_pid().as_u64()
}

fn sys_unlink(path: u64) -> u64 {
    todo!()
}

fn sys_rmdir(path: u64) -> u64 {
    todo!()
}
