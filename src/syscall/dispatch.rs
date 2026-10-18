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

use crate::tty::{GLOBAL_TTY, TCGETS, TCSETS, TCSETSF, TCSETSW, TIOCGWINSZ, Termios, Winsize};


pub(crate) fn dispatch(number: u64, args: [u64; 6], frame: &mut entry::SyscallFrame) -> SyscallResult {
    let Ok(syscall) = Syscall::try_from(number) else {
        return Err(Errno::NoSys);
    };

    match syscall {
        Syscall::Read => sys_read(args[0], args[1], args[2]),
        Syscall::Write => sys_write(args[0], args[1], args[2]),
        Syscall::Open => sys_open(args[0], args[1]),
        Syscall::Close => sys_close(args[0]),
        Syscall::Fstat => sys_fstat(args[0], args[1]),
        Syscall::Lseek => sys_lseek(args[0], args[1] as i64, args[2] as u32),
        Syscall::Mmap => sys_mmap(args[0], args[1], args[2], args[3], args[4], args[5]),
        Syscall::Munmap => sys_mun_map(args[0], args[1]),
        Syscall::Brk => sys_brk(args[0]),
        Syscall::Nanosleep => sys_nanosleep(args[0], args[1]),
        Syscall::Writev => sys_writev(args[0], args[1], args[2]),
        Syscall::Ioctl => sys_ioctl(args[0], args[1], args[2]),
        Syscall::Fork => sys_fork(frame),
        Syscall::Exit => sys_exit(args[0]),
        Syscall::ExitGroup => sys_exit(args[0]),
        Syscall::Wait4 => sys_wait4(args[0] as i64, args[1], args[2], args[3]),
        Syscall::ArchPrctl => sys_arch_prctl(args[0], args[1]),
        Syscall::Getpid => sys_getpid(),
        Syscall::SchedYield => sys_yield(),
        Syscall::Getdents64 => sys_getdents(args[0], args[1], args[2]),
        Syscall::SetTidAddress => sys_set_tid_address(args[0]),
        Syscall::Mkdir => sys_mkdir(args[0]),
        Syscall::Create => sys_create(args[0]),
        Syscall::Execve => sys_execve(args[0], args[1], args[2]),
        Syscall::Poll => sys_poll(args[0], args[1], args[2]),
        Syscall::Unlink => sys_unlink(args[0]),
        Syscall::Rmdir => sys_rmdir(args[0]),
        Syscall::RtSigaction => sys_rt_sigaction(args[0], args[1], args[2], args[3]),
        Syscall::RtSigprocmask => sys_rt_sigprocmask(args[0], args[1], args[2], args[3]),
        Syscall::RtSigreturn => sys_rt_sigreturn(frame),
        Syscall::Kill => sys_kill(args[0] as i64, args[1]),
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn sys_nanosleep(req_ptr: u64, _rem: u64) -> SyscallResult {
    let mut req = Timespec::default();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            &mut req as *mut Timespec as *mut u8,
            core::mem::size_of::<Timespec>(),
        )
    };
    if entry::copy_from_user(bytes, req_ptr).is_err() {
        return Err(Errno::Fault);
    }
    if req.tv_sec < 0 || req.tv_nsec < 0 || req.tv_nsec >= 1_000_000_000 {
        return Err(Errno::Invalid);
    }
    let ms = (req.tv_sec as u64) * 1000 + (req.tv_nsec as u64) / 1_000_000;
    let ticks = ms.div_ceil(crate::arch::interrupts::consts::TICK_MS);
    for _ in 0..ticks {
        if process::has_pending_signals() {
            return Err(Errno::Intr)
        }
        scheduler::sleep(1);
    }
    Ok(0)
}

fn sys_fstat(fd: u64, stat_buf: u64) -> SyscallResult {
    let mut stat = [0u8; 144];

    if fd <= 2 {
        stat[16..24].copy_from_slice(&1u64.to_le_bytes());
        stat[24..28].copy_from_slice(&0o020666u32.to_le_bytes());
        stat[56..64].copy_from_slice(&1024u64.to_le_bytes());
    } else {
        let (size, is_dir, ino) = process::with_current_fds(|fds| {
            let file = fds.get(fd).ok()?;
            let inode = file.inode.lock();
            let is_dir = inode.file_type == crate::storage::vfs::inode::FileType::Directory;
            Some((inode.size, is_dir, inode.id))
        })
        .flatten()
        .ok_or(Errno::BadFd)?;

        let mode = if is_dir { 0o040755u32 } else { 0o100644u32 };
        let blocks = (size + 511) / 512;

        stat[8..16].copy_from_slice(&ino.to_le_bytes());
        stat[16..24].copy_from_slice(&1u64.to_le_bytes());
        stat[24..28].copy_from_slice(&mode.to_le_bytes());
        stat[48..56].copy_from_slice(&size.to_le_bytes());
        stat[56..64].copy_from_slice(&4096u64.to_le_bytes());
        stat[64..72].copy_from_slice(&blocks.to_le_bytes());
    }

    entry::copy_to_user(stat_buf, &stat).map_err(|_| Errno::Fault)?;
    Ok(0)
}

fn sys_writev(fd: u64, iov_ptr: u64, iovcnt: u64) -> SyscallResult {
    if iovcnt > 1024 {
        return Err(Errno::Invalid);
    }

    let mut total_written = 0;

    for i in 0..iovcnt {
        let slot = iov_ptr.checked_add(i * 16).ok_or(Errno::Fault)?;
        let mut iov_buf = [0u8; 16];
        entry::copy_from_user(&mut iov_buf, slot).map_err(|_| Errno::Fault)?;

        let base = u64::from_le_bytes(iov_buf[0..8].try_into().unwrap());
        let len = u64::from_le_bytes(iov_buf[8..16].try_into().unwrap());

        if len == 0 {
            continue;
        }
        total_written += sys_write(fd, base, len)?;
    }
    Ok(total_written)
}

fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, _fd: u64, _offset: u64) -> SyscallResult {
    let pid = current_pid();
    match process::mmap(pid, addr, len, prot, flags) {
        Ok(mapped_addr) => Ok(mapped_addr),
        Err(_) => Err(Errno::NoMem),
    }
}

fn sys_mun_map(_addr: u64, _len: u64) -> SyscallResult {
    Ok(0)
}

fn sys_ioctl(fd: u64, request: u64, arg: u64) -> SyscallResult {
    // Only fds 0, 1, 2 are terminal devices
    if fd > 2 {
        return Err(Errno::NotTty);
    }

    match request {
        TCGETS => {
            let termios = GLOBAL_TTY.lock().termios;
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &termios as *const Termios as *const u8,
                    core::mem::size_of::<Termios>(),
                )
            };
            if entry::copy_to_user(arg, bytes).is_err() {
                return Err(Errno::Fault);
            }
            Ok(0)
        }
        TCSETS | TCSETSW | TCSETSF => {
            let mut termios = Termios::default();
            let bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    &mut termios as *mut Termios as *mut u8,
                    core::mem::size_of::<Termios>(),
                )
            };
            if entry::copy_from_user(bytes, arg).is_err() {
                return Err(Errno::Fault);
            }
            GLOBAL_TTY.lock().termios = termios;
            Ok(0)
        }
        TIOCGWINSZ => {
            let (rows, cols) = {
                let writer = crate::graphics::framebuffer::WRITER.lock();
                (writer.rows() as u16, writer.cols() as u16)
            };
            let win = Winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &win as *const Winsize as *const u8,
                    core::mem::size_of::<Winsize>(),
                )
            };
            if entry::copy_to_user(arg, bytes).is_err() {
                return Err(Errno::Fault);
            }
            Ok(0)
        }
        crate::tty::TIOCGPGRP => {
            let pgid = GLOBAL_TTY.lock().foreground_pgrp.map(|p| p.as_u64() as i32).unwrap_or(0);
            entry::copy_to_user(arg, &pgid.to_le_bytes()).map_err(|_| Errno::Fault)?;
            Ok(0)
        }
        crate::tty::TIOCSPGRP => {
            let mut pgid_bytes = [0u8; 4];
            entry::copy_from_user(&mut pgid_bytes, arg).map_err(|_| Errno::Fault)?;
            let pgid = i32::from_le_bytes(pgid_bytes);
            GLOBAL_TTY.lock().foreground_pgrp = Some(crate::task::identity::ProcessId(pgid as u64));
            Ok(0)
        }
        _ => Err(Errno::Invalid),
    }
}
fn sys_set_tid_address(_tidptr: u64) -> SyscallResult {
    Ok(current_pid().as_u64())
}
fn sys_brk(addr: u64) -> SyscallResult {
    let pid = current_pid();
    match process::brk(pid, addr) {
        Ok(current_break) => Ok(current_break),
        Err(_) => Err(Errno::NoMem),
    }
}
fn sys_fork(frame: &entry::SyscallFrame) -> SyscallResult {
    let parent_pid = current_pid();

    let child_pid = process::create_child(parent_pid).ok_or(Errno::NoMem)?;

    let Some(root) = process::space_root_of(parent_pid) else {
        return Err(Errno::NoMem);
    };

    let child_space =
        crate::memory::mmu::AddressSpace::clone_from_root(root).map_err(|_| Errno::NoMem)?;
    process::attach_space(child_pid, child_space.root);
    core::mem::forget(child_space);

    let child_thread = crate::task::thread::Thread::new_forked(
        child_pid,
        "child",
        crate::task::identity::Priority::Normal,
        frame,
    );

    scheduler::add_forked_thread(child_thread);

    Ok(child_pid.as_u64())
}

fn sys_wait4(pid: i64, wstatus: u64, _options: u64, _rusage: u64) -> SyscallResult {
    let Some((child_pid, code)) = process::wait4(pid) else {
        return Err(Errno::Child);
    };

    if wstatus != 0 {
        let status = ((code & 0xff) << 8) as i32;
        if entry::copy_to_user(wstatus, &status.to_le_bytes()).is_err() {
            return Err(Errno::Fault);
        }
    }

    Ok(child_pid.as_u64())
}
const MAX_ARGS: usize = 64;

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
    let mut total_written = 0;
    let target = len as usize;
    let mut staging = [0u8; 4096];

    while total_written < target {
        let chunk = core::cmp::min(target - total_written, 4096);
        let curr_buf = buf + total_written as u64;

        if entry::copy_from_user(&mut staging[..chunk], curr_buf).is_err() {
            return Err(Errno::Fault);
        }

        let Some(result) = process::with_current_fds(|fds| fds.write(fd, &staging[..chunk])) else {
            return Err(Errno::BadFd);
        };

        match result {
            Ok(n) if n > 0 => total_written += n,
            Ok(_) => break,
            Err(_) => return Err(Errno::Invalid),
        }
    }

    Ok(total_written as u64)
}
pub(crate) fn wait_for_key() {
    let id = crate::task::identity::current_thread_id();

    crate::arch::input::add_waiter(id);

    let switched = scheduler::block_current();

    if !switched {
        x86_64::instructions::interrupts::enable_and_hlt();
        x86_64::instructions::interrupts::disable();
    }

    crate::arch::input::remove_waiter(id);
}

fn sys_poll(fds: u64, nfds: u64, _events: u64) -> SyscallResult {
    if nfds == 0 {
        return Err(Errno::Invalid);
    }

    let mut revents = [0u8; 8];

    if crate::tty::GLOBAL_TTY.lock().queue.len() > 0 {
        revents[0] = 1;
    }

    if entry::copy_to_user(fds, &revents).is_err() {
        return Err(Errno::Fault);
    }

    Ok(if revents[0] != 0 { 1 } else { 0 })
}

fn sys_read(fd: u64, buf: u64, len: u64) -> SyscallResult {
    if len == 0 {
        return Ok(0);
    }

    if len > 4096 {
        return Err(Errno::Invalid);
    }

    let file = process::with_current_fds(|fds| fds.get(fd).cloned());
    let Some(Ok(mut file)) = file else {
        return Err(Errno::BadFd);
    };

    let mut staging = [0u8; 4096];
    let to_read = (len as usize).min(staging.len());

    let is_dev = file.inode.lock().file_type == crate::storage::vfs::inode::FileType::Device;

    let read = if is_dev {
        match file.read(&mut staging[..to_read]) {
            Ok(read) => read,
            Err("interrupted") => return Err(Errno::Intr),
            Err(_) => return Err(Errno::Invalid),
        }
    } else {
        let Some(result) = process::with_current_fds(|fds| fds.read(fd, &mut staging[..to_read]))
        else {
            return Err(Errno::BadFd);
        };
        match result {
            Ok(read) => read,
            Err("not a regular or device file") => return Err(Errno::IsDir),
            Err(_) => return Err(Errno::Invalid),
        }
    };

    if read > 0 && entry::copy_to_user(buf, &staging[..read]).is_err() {
        return Err(Errno::Fault);
    }

    Ok(read as u64)
}

fn sys_open(path: u64, flags: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };

    const O_ACCMODE: u64 = 0x3;
    const O_WRONLY: u64 = 1;
    const O_RDWR: u64 = 2;
    const O_CREAT: u64 = 0x40;
    const O_TRUNC: u64 = 0x200;
    const O_APPEND: u64 = 0x400;
    const O_DIRECTORY: u64 = 0x10000;

    let acc = flags & O_ACCMODE;
    let read = acc == 0 || acc == O_RDWR;
    let write = acc == O_WRONLY || acc == O_RDWR;
    let create = (flags & O_CREAT) != 0;
    let truncate = (flags & O_TRUNC) != 0;
    let append = (flags & O_APPEND) != 0;
    let directory = (flags & O_DIRECTORY) != 0;

    let open_flags = OpenFlags {
        read,
        write,
        create,
        truncate,
        append,
        directory,
    };

    let Some(result) = process::with_current_fds(|fds| fds.open(&path, open_flags)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(fd) => Ok(fd),
        Err("not a directory") => Err(Errno::NotDir),
        Err("is a directory") => Err(Errno::IsDir),
        Err(_) => Err(Errno::NoEntry),
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

fn sys_arch_prctl(code: u64, addr: u64) -> SyscallResult {
    const ARCH_SET_FS: u64 = 0x1002;

    if code != ARCH_SET_FS {
        return Err(Errno::Invalid);
    }
    if addr < USER_START || addr >= USER_END {
        return Err(Errno::Fault);
    }
    x86_64::registers::model_specific::FsBase::write(VirtAddr::new(addr));

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

fn sys_kill(pid: i64, sig: u64) -> SyscallResult {
    if sig > 64 {
        return Err(Errno::Invalid);
    }
    if pid <= 0 {
        return Err(Errno::Invalid);
    }
    crate::task::process::send_signal(crate::task::identity::ProcessId(pid as u64), sig as usize);
    Ok(0)
}

fn sys_rt_sigaction(sig: u64, act_ptr: u64, oldact_ptr: u64, sigsetsize: u64) -> SyscallResult {
    if sig == 0 || sig > 64 || sigsetsize != 8 {
        return Err(Errno::Invalid);
    }
    if sig == 9 || sig == 19 {
        return Err(Errno::Invalid);
    }

    let proc = process::current().ok_or(Errno::Fault)?;
    let mut p = proc.lock();
    let idx = (sig - 1) as usize;

    if oldact_ptr != 0 {
        let old = p.sigactions[idx];
        let bytes = unsafe {
            core::slice::from_raw_parts(
                &old as *const _ as *const u8,
                core::mem::size_of::<crate::task::signal::SigAction>(),
            )
        };
        entry::copy_to_user(oldact_ptr, bytes).map_err(|_| Errno::Fault)?;
    }

    if act_ptr != 0 {
        let mut act = crate::task::signal::SigAction::default();
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                &mut act as *mut _ as *mut u8,
                core::mem::size_of::<crate::task::signal::SigAction>(),
            )
        };
        entry::copy_from_user(bytes, act_ptr).map_err(|_| Errno::Fault)?;
        p.sigactions[idx] = act;
    }

    Ok(0)
}

fn sys_rt_sigprocmask(how: u64, set_ptr: u64, oldset_ptr: u64, sigsetsize: u64) -> SyscallResult {
    if sigsetsize != 8 {
        return Err(Errno::Invalid);
    }

    let proc = process::current().ok_or(Errno::Fault)?;
    let mut p = proc.lock();

    if oldset_ptr != 0 {
        let old = p.blocked_signals;
        entry::copy_to_user(oldset_ptr, &old.to_le_bytes()).map_err(|_| Errno::Fault)?;
    }

    if set_ptr != 0 {
        let mut new_set_bytes = [0u8; 8];
        entry::copy_from_user(&mut new_set_bytes, set_ptr).map_err(|_| Errno::Fault)?;
        let set = u64::from_le_bytes(new_set_bytes);

        const SIG_BLOCK: u64 = 0;
        const SIG_UNBLOCK: u64 = 1;
        const SIG_SETMASK: u64 = 2;

        match how {
            SIG_BLOCK => p.blocked_signals |= set,
            SIG_UNBLOCK => p.blocked_signals &= !set,
            SIG_SETMASK => p.blocked_signals = set,
            _ => return Err(Errno::Invalid),
        }

        // SIGKILL and SIGSTOP cannot be blocked
        p.blocked_signals &= !((1u64 << (9 - 1)) | (1u64 << (19 - 1)));
    }

    Ok(0)
}

fn sys_rt_sigreturn(frame: &mut entry::SyscallFrame) -> SyscallResult {
    let mut ctx = core::mem::MaybeUninit::<crate::task::signal::SignalUserContext>::uninit();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            ctx.as_mut_ptr() as *mut u8,
            core::mem::size_of::<crate::task::signal::SignalUserContext>(),
        )
    };

    entry::copy_from_user(bytes, frame.rsp).map_err(|_| Errno::Fault)?;
    let ctx = unsafe { ctx.assume_init() };

    frame.regs = ctx.regs;
    frame.rip = ctx.rip;
    frame.rsp = ctx.rsp;
    frame.rflags = ctx.rflags;

    if let Some(proc) = process::current() {
        proc.lock().blocked_signals = ctx.mask;
    }

    Ok(frame.regs.rax)
}
