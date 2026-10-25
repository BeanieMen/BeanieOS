use alloc::collections::VecDeque;
use alloc::vec::Vec;
use alloc::{string::String, sync::Arc};
use pc_keyboard::KeyCode::E;
use spin::mutex::Mutex;

use crate::input::event::EVENT_FILE_OPS;
use crate::net::unix::SocketState::Closed;
use crate::net::unix::UnixSocket;
use crate::net::vfs::{SOCKET_REGISTRY, create_connected_pair, create_socket_file};
use crate::syscall::entry::{copy_from_user, copy_to_user, user_path};
use crate::task::process::{has_pending_signals, with_current_fds};
use crate::task::scheduler::{sleep, yield_now};
use crate::{
    arch::interrupts::{consts::TICK_MS, vectors::ticks},
    kdebug, kerror,
    memory::mmu::USER_END,
    storage::vfs::file::OpenFlags,
    task::{identity::current_pid, process, scheduler},
};
use x86_64::VirtAddr;

use super::entry;
use super::error::{Errno, SyscallResult};
use super::numbers::*;

use crate::tty::{GLOBAL_TTY, TCGETS, TCSETS, TCSETSF, TCSETSW, TIOCGWINSZ, Termios, Winsize};

pub(crate) fn dispatch(
    number: u64,
    args: [u64; 6],
    frame: &mut entry::SyscallFrame,
) -> SyscallResult {
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
        Syscall::Readv => sys_readv(args[0], args[1], args[2]),
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
        Syscall::ClockGettime => sys_clock_gettime(args[0], args[1]),
        Syscall::Socket => sys_socket(args[0], args[1], args[2]),
        Syscall::Connect => sys_connect(args[0], args[1], args[2]),
        Syscall::Accept => sys_accept(args[0], args[1], args[2], 0),
        Syscall::Accept4 => sys_accept(args[0], args[1], args[2], args[3]),
        Syscall::Dup => sys_dup(args[0]),
        Syscall::Dup2 => sys_dup2(args[0], args[1]),
        Syscall::Dup3 => sys_dup3(args[0], args[1], args[2]),
        Syscall::Bind => sys_bind(args[0], args[1], args[2]),
        Syscall::Listen => sys_listen(args[0], args[1]),
        Syscall::Socketpair => sys_socketpair(args[0], args[1], args[2], args[3]),
        Syscall::Select => sys_select(args[0], args[1], args[2], args[3], args[4]),
        Syscall::Pselect6 => sys_select(args[0], args[1], args[2], args[3], args[4]),
        Syscall::Shutdown => sys_shutdown(args[0], args[1]),
        Syscall::Getsockname => sys_getsockname(args[0], args[1], args[2]),
        Syscall::Getpeername => sys_getpeername(args[0], args[1], args[2]),
        Syscall::Setsockopt => sys_setsockopt(args[0], args[1], args[2], args[3], args[4]),
        Syscall::Getsockopt => sys_getsockopt(args[0], args[1], args[2], args[3], args[4]),
        Syscall::Uname => sys_uname(args[0]),
        Syscall::Fcntl => sys_fcntl(args[0], args[1], args[2]),
        Syscall::Getuid | Syscall::Getgid | Syscall::Geteuid | Syscall::Getegid => Ok(0),
        Syscall::Chmod | Syscall::Fchmod | Syscall::Chown | Syscall::Fchown => Ok(0),
        Syscall::Pipe => sys_pipe(args[0]),
        Syscall::Pipe2 => sys_pipe2(args[0], args[1]),
        Syscall::Openat => sys_openat(args[0], args[1], args[2]),
        Syscall::Readlink => sys_readlink(args[0], args[1], args[2]),
        Syscall::Readlinkat => sys_readlinkat(args[0], args[1], args[2], args[3]),
        Syscall::Getcwd => sys_getcwd(args[0], args[1]),
        Syscall::Madavise => Ok(0),
        Syscall::Newfstatat => sys_newfstatat(args[0], args[1], args[2], args[3]),
        Syscall::Ultimensat => Ok(0),
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn sys_newfstatat(dirfd: u64, path_ptr: u64, stat_buf: u64, _flags: u64) -> SyscallResult {
    const AT_FDCWD: i64 = -100;
    if dirfd as i64 != AT_FDCWD && dirfd != 0 {
        return Err(Errno::Fault);
    }

    let Ok(path) = user_path(path_ptr) else {
        return Err(Errno::Fault);
    };

    let resolved_path = resolve_relative_path(&path);
    let open_flags = OpenFlags::READ;
    let fd = with_current_fds(|fds| fds.open(&resolved_path, open_flags)).ok_or(Errno::BadFd)?.map_err(|_| Errno::NoEntry)?;
    let res = sys_fstat(fd, stat_buf);
    let _ = with_current_fds(|fds| fds.close(fd));
    res
}

fn sys_getcwd(buf_ptr: u64, size: u64) -> SyscallResult { 
    let cwd = b"/\0";
    if size < cwd.len() as u64 {
        return Err(Errno::Range);
    }

    copy_to_user(buf_ptr,cwd).map_err(|_| Errno::Fault)?;
    Ok(buf_ptr)
}

fn sys_openat(dirfd: u64, path: u64, flags: u64) -> SyscallResult {
    const AT_FDCWD: i64 = -100;
    if dirfd as i64 != AT_FDCWD {
        return Err(Errno::Invalid);
    }
    sys_open(path, flags)
}

fn sys_readlink(_path: u64, _buf: u64, _bufsiz: u64) -> SyscallResult {
    Err(Errno::Invalid)
}

fn sys_readlinkat(_dirfd: u64, path: u64, buf: u64, bufsiz: u64) -> SyscallResult {
    sys_readlink(path, buf, bufsiz)
}

fn sys_pipe(pipefd_ptr: u64) -> SyscallResult {
    sys_pipe2(pipefd_ptr, 0)
}

fn sys_pipe2(pipefd_ptr: u64, flags: u64) -> SyscallResult {
    const O_CLOEXEC: u64 = 0x80000;
    const O_NONBLOCK: u64 = 0x800;
    if flags & !(O_CLOEXEC | O_NONBLOCK) != 0 {
        return Err(Errno::Invalid);
    }

    let (a, b) = create_connected_pair();
    let f1 = create_socket_file(a);
    let f2 = create_socket_file(b);
    let (fd1, fd2) = with_current_fds(|fds| {
        let fd1 = fds.alloc(f1);
        let fd2 = fds.alloc(f2);
        (fd1 as i32, fd2 as i32)
    })
    .ok_or(Errno::NoMem)?;

    let mut out = [0u8; 8];
    out[0..4].copy_from_slice(&fd1.to_le_bytes());
    out[4..8].copy_from_slice(&fd2.to_le_bytes());
    copy_to_user(pipefd_ptr, &out).map_err(|_| Errno::Fault)?;
    Ok(0)
}

fn sys_dup(oldfd: u64) -> SyscallResult {
    let file = process::with_current_fds(|fds| fds.get(oldfd).cloned())
        .ok_or(Errno::BadFd)?
        .map_err(|_| Errno::BadFd)?;
    let newfd = process::with_current_fds(|fds| fds.alloc(file)).ok_or(Errno::BadFd)?;
    Ok(newfd)
}

fn sys_dup2(oldfd: u64, newfd: u64) -> SyscallResult {
    let Some(result) = with_current_fds(|fds| fds.dup2(oldfd, newfd)) else {
        return Err(Errno::BadFd);
    };

    result.map_err(|_| Errno::BadFd)
}

fn sys_dup3(oldfd: u64, newfd: u64, flags: u64) -> SyscallResult {
    if oldfd == newfd {
        return Err(Errno::Invalid);
    }
    const O_CLOEXEC: u64 = 0x80000;
    if flags & !O_CLOEXEC != 0 {
        return Err(Errno::Invalid);
    }
    sys_dup2(oldfd, newfd)
}

fn sys_fcntl(fd: u64, cmd: u64, arg: u64) -> SyscallResult {
    let f = with_current_fds(|fds| fds.get(fd).cloned().ok())
        .flatten()
        .ok_or(Errno::BadFd)?;
    match cmd {
        0 => {
            // F_DUPFD
            let min_fd = arg as usize;
            with_current_fds(|fds| fds.alloc_at_least(f.clone(), min_fd)).ok_or(Errno::BadFd)
        }
        1 => Ok(0), // F_GETFD
        2 => Ok(0), // F_SETFD
        3 => {
            // F_GETFL
            let flags = f.flags;
            let mut val = 0u64;
            if flags.read && flags.write {
                val |= 2;
            } else if flags.write {
                val |= 1;
            }
            if flags.append {
                val |= 0x400;
            }
            if flags.nonblock {
                val |= 0x800;
            }
            Ok(val)
        }
        4 => {
            // F_SETFL
            let nonblock = (arg & 0x800) != 0 || (arg & 0x4) != 0;
            with_current_fds(|fds| {
                if let Ok(f) = fds.get_mut(fd) {
                    f.flags.nonblock = nonblock;
                }
            });
            Ok(0)
        }
        _ => Ok(0),
    }
}

fn sys_uname(buf_ptr: u64) -> SyscallResult {
    // struct utsname: 6 fields of 65 bytes each
    let mut uts = [0u8; 65 * 6];
    let fill = |slice: &mut [u8], s: &[u8]| {
        let len = s.len().min(slice.len() - 1);
        slice[..len].copy_from_slice(&s[..len]);
    };

    fill(&mut uts[0..65], b"Linux"); // sysname
    fill(&mut uts[65..130], b"beanieos"); // nodename
    fill(&mut uts[130..195], b"6.1.0-beanieos"); // release
    fill(&mut uts[195..260], b"#1 SMP PREEMPT"); // version
    fill(&mut uts[260..325], b"x86_64"); // machine
    fill(&mut uts[325..390], b"local"); // domainname

    copy_to_user(buf_ptr, &uts).map_err(|_| Errno::Fault)?;
    Ok(0)
}

fn sys_setsockopt(
    _fd: u64,
    _level: u64,
    _optname: u64,
    _optval: u64,
    _optlen: u64,
) -> SyscallResult {
    // Xfbdev sets SO_REUSEADDR on its socket; returning Ok allows it to bind cleanly
    Ok(0)
}

fn sys_getsockopt(_fd: u64, _level: u64, _optname: u64, optval: u64, optlen: u64) -> SyscallResult {
    if optval != 0 && optlen != 0 {
        let zero = 0u32;
        copy_to_user(optval, &zero.to_le_bytes()).map_err(|_| Errno::Fault)?;
    }
    Ok(0)
}
fn sys_getsockname(fd: u64, addr_ptr: u64, addrlen_ptr: u64) -> SyscallResult {
    let sock = get_socket_by_fd(fd)?;
    let s = sock.lock();
    if let Some(ref path) = s.bound_path {
        if path.len() == 1 && path[0] == 2 {
            let mut sin = [0u8; 16];
            sin[0..2].copy_from_slice(&2u16.to_le_bytes()); // AF_INET
            sin[2] = 0x17; // port 6000 network order
            sin[3] = 0x70;
            copy_to_user(addr_ptr, &sin).map_err(|_| Errno::Fault)?;
            let len = 16u32;
            copy_to_user(addrlen_ptr, &len.to_le_bytes()).map_err(|_| Errno::Fault)?;
            return Ok(0);
        }

        let mut raw = [0u8; 110];
        raw[0..2].copy_from_slice(&1u16.to_le_bytes()); // AF_UNIX

        let copy_len = path.len().min(107);

        raw[2..2 + copy_len].copy_from_slice(&path[..copy_len]);
        copy_to_user(addr_ptr, &raw).map_err(|_| Errno::Fault)?;
        let len = (2 + copy_len + 1) as u32;
        copy_to_user(addrlen_ptr, &len.to_le_bytes()).map_err(|_| Errno::Fault)?;
        Ok(0)
    } else {
        let mut sin = [0u8; 16];
        sin[0..2].copy_from_slice(&2u16.to_le_bytes());
        copy_to_user(addr_ptr, &sin).map_err(|_| Errno::Fault)?;
        let len = 16u32;
        copy_to_user(addrlen_ptr, &len.to_le_bytes()).map_err(|_| Errno::Fault)?;
        Ok(0)
    }
}

fn sys_getpeername(fd: u64, addr_ptr: u64, addrlen_ptr: u64) -> SyscallResult {
    sys_getsockname(fd, addr_ptr, addrlen_ptr)
}

fn sys_shutdown(fd: u64, _how: u64) -> SyscallResult {
    let sock = get_socket_by_fd(fd)?;
    sock.lock().state = Closed;
    Ok(0)
}
fn sys_socket(domain: u64, type_: u64, _proto: u64) -> SyscallResult {
    if (domain != 1 && domain != 2) || (type_ & 0xF) != 1 {
        return Err(Errno::Invalid);
    }
    let sock = Arc::new(Mutex::new(UnixSocket::new()));
    let mut file = create_socket_file(sock);
    if (type_ & 0x800) != 0 {
        file.flags.nonblock = true;
    }
    let fd = process::with_current_fds(|fds| fds.alloc(file)).ok_or(Errno::NoMem)?;
    Ok(fd)
}

fn sys_bind(fd: u64, addr_ptr: u64, _len: u64) -> SyscallResult {
    let mut raw_addr = [0u8; 110];
    copy_from_user(&mut raw_addr, addr_ptr).map_err(|_| Errno::Fault)?;

    let family = u16::from_le_bytes([raw_addr[0], raw_addr[1]]);
    if family == 2 {
        let sock = get_socket_by_fd(fd)?;
        sock.lock().bound_path = Some(alloc::vec![2]);
        return Ok(0);
    }

    let path = parse_sun_path(&raw_addr[2..]);
    let sock = get_socket_by_fd(fd)?;
    let mut reg = SOCKET_REGISTRY.lock();
    if reg.contains_key(&path) {
        return Err(Errno::Invalid);
    }

    sock.lock().bound_path = Some(path.clone());
    reg.insert(path, sock);
    Ok(0)
}

fn sys_accept(fd: u64, addr_ptr: u64, len_ptr: u64, flags: u64) -> SyscallResult {
    let sock = get_socket_by_fd(fd)?;
    let is_nonblock = {
        let f =
            process::with_current_fds(|fds| fds.get(fd).map(|f| f.flags.nonblock).unwrap_or(false))
                .unwrap_or(false);
        f || (flags & 0x800) != 0
    };

    loop {
        let conn = {
            let mut s = sock.lock();
            if let crate::net::unix::SocketState::Listening { incoming, .. } = &mut s.state {
                incoming.pop_front()
            } else {
                return Err(Errno::Invalid);
            }
        };

        if let Some(conn_sock) = conn {
            let mut file = create_socket_file(conn_sock);
            if (flags & 0x800) != 0 {
                file.flags.nonblock = true;
            }
            let new_fd = with_current_fds(|fds| fds.alloc(file)).ok_or(Errno::NoMem)?;

            if addr_ptr != 0 && len_ptr != 0 {
                let mut ulen_bytes = [0u8; 4];
                if entry::copy_from_user(&mut ulen_bytes, len_ptr).is_ok() {
                    let ulen = u32::from_le_bytes(ulen_bytes);
                    let mut raw = [0u8; 110];
                    raw[0..2].copy_from_slice(&1u16.to_le_bytes()); // AF_UNIX
                    let copy_len = (ulen as usize).min(110);
                    let _ = entry::copy_to_user(addr_ptr, &raw[..copy_len]);
                    let len_out = 2u32;
                    let _ = entry::copy_to_user(len_ptr, &len_out.to_le_bytes());
                }
            }

            return Ok(new_fd);
        }

        if is_nonblock {
            return Err(Errno::Again);
        }

        if crate::task::process::has_pending_signals() {
            return Err(Errno::Intr);
        }

        yield_now();
    }
}

fn sys_socketpair(_domain: u64, _type: u64, _proto: u64, sv_ptr: u64) -> SyscallResult {
    let (a, b) = create_connected_pair();
    let f1 = create_socket_file(a);
    let f2 = create_socket_file(b);
    let (fd1, fd2) = with_current_fds(|fds| {
        let fd1 = fds.alloc(f1);
        let fd2 = fds.alloc(f2);
        (fd1 as i32, fd2 as i32)
    })
    .ok_or(Errno::NoMem)?;

    let mut out = [0u8; 8];
    out[0..4].copy_from_slice(&fd1.to_le_bytes());
    out[4..8].copy_from_slice(&fd2.to_le_bytes());
    copy_to_user(sv_ptr, &out).map_err(|_| Errno::Fault)?;
    Ok(0)
}
fn sys_listen(fd: u64, backlog: u64) -> SyscallResult {
    let sock = get_socket_by_fd(fd)?;
    let mut s = sock.lock();
    s.state = crate::net::unix::SocketState::Listening {
        backlog: backlog as usize,
        incoming: VecDeque::new(),
    };

    Ok(0)
}

fn sys_connect(fd: u64, addr_ptr: u64, _len: u64) -> SyscallResult {
    let mut raw_addr = [0u8; 110];
    entry::copy_from_user(&mut raw_addr, addr_ptr).map_err(|_| Errno::Fault)?;
    let path = parse_sun_path(&raw_addr[2..]);
    let server_sock = {
        let reg = SOCKET_REGISTRY.lock();
        reg.get(&path).cloned().ok_or(Errno::NoEntry)?
    };
    let client_sock = get_socket_by_fd(fd)?;
    let server_side = alloc::sync::Arc::new(spin::Mutex::new(crate::net::unix::UnixSocket::new()));
    client_sock.lock().state = crate::net::unix::SocketState::Connected {
        peer: alloc::sync::Arc::downgrade(&server_side),
        rx_buffer: alloc::collections::VecDeque::new(),
        peer_closed: false,
    };
    server_side.lock().state = crate::net::unix::SocketState::Connected {
        peer: alloc::sync::Arc::downgrade(&client_sock),
        rx_buffer: alloc::collections::VecDeque::new(),
        peer_closed: false,
    };
    let mut s = server_sock.lock();
    if let crate::net::unix::SocketState::Listening { incoming, backlog } = &mut s.state {
        if incoming.len() < *backlog {
            incoming.push_back(server_side);
            Ok(0)
        } else {
            Err(Errno::Invalid)
        }
    } else {
        Err(Errno::Invalid)
    }
}

fn sys_clock_gettime(_clock_id: u64, tp_ptr: u64) -> SyscallResult {
    let ticks = ticks();
    let total_ms = ticks * TICK_MS;
    let sec = (total_ms / 1000) as i64;
    let nsec = ((total_ms) % 1000 * 1_000_000) as i64;

    let timespec = Timespec {
        tv_sec: sec,
        tv_nsec: nsec,
    };

    let bytes = unsafe {
        core::slice::from_raw_parts(
            &timespec as *const Timespec as *const u8,
            core::mem::size_of::<Timespec>(),
        )
    };
    entry::copy_to_user(tp_ptr, bytes).map_err(|_| Errno::Fault)?;
    Ok(0)
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
    if ticks > 0 {
        if process::has_pending_signals() {
            return Err(Errno::Intr);
        }
        scheduler::sleep(ticks);
        if process::has_pending_signals() {
            return Err(Errno::Intr);
        }
    }
    Ok(0)
}

fn sys_readv(fd: u64, iov_ptr: u64, iovcnt: u64) -> SyscallResult {
    if iovcnt > 1024 {
        return Err(Errno::Invalid);
    }

    let mut total_read = 0;

    for i in 0..iovcnt {
        let slot = iov_ptr.checked_add(i * 16).ok_or(Errno::Fault)?;
        let mut iov_buf = [0u8; 16];
        entry::copy_from_user(&mut iov_buf, slot).map_err(|_| Errno::Fault)?;

        let base = u64::from_le_bytes(iov_buf[0..8].try_into().unwrap());
        let len = u64::from_le_bytes(iov_buf[8..16].try_into().unwrap());
        if len == 0 {
            continue;
        }

        let n = match sys_read(fd, base, len) {
            Ok(n) => n,
            Err(e) => {
                if total_read > 0 {
                    return Ok(total_read);
                }
                return Err(e);
            }
        };

        total_read += n;
        if n < len {
            break;
        }
    }
    Ok(total_read)
}

fn sys_fstat(fd: u64, stat_buf: u64) -> SyscallResult {
    let mut stat = [0u8; 144];

    if fd <= 2 {
        stat[16..24].copy_from_slice(&1u64.to_le_bytes());
        stat[24..28].copy_from_slice(&0o020666u32.to_le_bytes());
        stat[56..64].copy_from_slice(&1024u64.to_le_bytes());
    } else {
        let (size, is_dir, is_dev, ino) = process::with_current_fds(|fds| {
            let file = fds.get(fd).ok()?;
            let inode = file.inode.lock();
            let is_dir = inode.file_type == crate::storage::vfs::inode::FileType::Directory;
            let is_dev = inode.file_type == crate::storage::vfs::inode::FileType::Device;
            Some((inode.size, is_dir, is_dev, inode.id))
        })
        .flatten()
        .ok_or(Errno::BadFd)?;

        let mode = if is_dir {
            0o040755u32
        } else if is_dev {
            0o020666u32
        } else {
            0o100644u32
        };
        let blocks = size.div_ceil(512);

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

    let mut file = process::with_current_fds(|fds| fds.get(fd).cloned())
        .and_then(|r| r.ok())
        .ok_or(Errno::BadFd)?;

    let mut total_written = 0;
    let mut staging = [0u8; 4096];

    for i in 0..iovcnt {
        let slot = iov_ptr.checked_add(i * 16).ok_or(Errno::Fault)?;
        let mut iov_buf = [0u8; 16];
        entry::copy_from_user(&mut iov_buf, slot).map_err(|_| Errno::Fault)?;

        let base = u64::from_le_bytes(iov_buf[0..8].try_into().unwrap());
        let len = u64::from_le_bytes(iov_buf[8..16].try_into().unwrap());

        if len == 0 {
            continue;
        }

        let target = len as usize;
        let mut written_for_iov = 0;

        while written_for_iov < target {
            let chunk = core::cmp::min(target - written_for_iov, 4096);
            let curr_buf = base + written_for_iov as u64;

            entry::copy_from_user(&mut staging[..chunk], curr_buf).map_err(|_| Errno::Fault)?;

            let n = match file.write(&staging[..chunk]) {
                Ok(n) => n,
                Err("broken pipe") => return Err(Errno::Pipe),
                Err(_) => return Err(Errno::Invalid),
            };
            if n == 0 {
                break;
            }
            written_for_iov += n;
            if scheduler::is_reschedule_requested() {
                scheduler::yield_now();
            }
        }

        total_written += written_for_iov as u64;
    }
    Ok(total_written)
}

fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, fd: u64, _offset: u64) -> SyscallResult {
    let pid = current_pid();
    let is_fb = fd >= 3
        && process::with_current_fds(|fds| {
            let file = fds.get(fd).ok()?;
            let inode = file.inode.lock();
            Some(core::ptr::addr_eq(
                inode.file_ops,
                &crate::graphics::vfs::FB_FILE_OPS,
            ))
        })
        .flatten()
        .unwrap_or(false);

    if is_fb {
        let (start, end) =
            crate::graphics::framebuffer::framebuffer_range().ok_or(Errno::Invalid)?;
        let size = (end - start).min(len);
        return process::mmap_phys(pid, start, size).map_err(|_| Errno::NoMem);
    }
    match process::mmap(pid, addr, len, prot, flags) {
        Ok(mapped_addr) => Ok(mapped_addr),
        Err(_) => Err(Errno::NoMem),
    }
}

fn sys_mun_map(addr: u64, len: u64) -> SyscallResult {
    if len == 0 {
        return Ok(0);
    }
    if addr % 4096 != 0 {
        return Err(Errno::Invalid);
    }
    let end = match addr.checked_add(len) {
        Some(v) => v,
        None => return Err(Errno::Invalid),
    };
    if end > crate::memory::mmu::USER_END {
        return Err(Errno::Invalid);
    }

    let pid = crate::task::identity::current_pid();
    let root = match crate::task::process::space_root_of(pid) {
        Some(r) => r,
        None => return Ok(0),
    };
    let mut space = crate::memory::mmu::AddressSpace::from_root(root);
    let mut curr = addr;
    while curr < end {
        let _ = space.unmap(curr);
        curr += 4096;
    }
    core::mem::forget(space);
    Ok(0)
}

fn sys_ioctl(_fd: u64, request: u64, arg: u64) -> SyscallResult {
    // Framebuffer ioctls (can be on any fd opened to /dev/fb0)
    match request {
        crate::graphics::fbdev::FBIOGET_VSCREENINFO
        | crate::graphics::fbdev::FBIOPUT_VSCREENINFO => {
            let writer = crate::graphics::framebuffer::WRITER.lock();
            let Some(ref fb) = writer.framebuffer else {
                return Err(Errno::Invalid);
            };
            let var = crate::graphics::fbdev::FbVarScreeninfo {
                xres: fb.width,
                yres: fb.height,
                xres_virtual: fb.width,
                yres_virtual: fb.height,
                bits_per_pixel: fb.bpp as u32,
                red: crate::graphics::fbdev::FbBitfield {
                    offset: 16,
                    length: 8,
                    msb_right: 0,
                },
                green: crate::graphics::fbdev::FbBitfield {
                    offset: 8,
                    length: 8,
                    msb_right: 0,
                },
                blue: crate::graphics::fbdev::FbBitfield {
                    offset: 0,
                    length: 8,
                    msb_right: 0,
                },
                ..Default::default()
            };
            drop(writer);

            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &var as *const _ as *const u8,
                    core::mem::size_of::<crate::graphics::fbdev::FbVarScreeninfo>(),
                )
            };
            entry::copy_to_user(arg, bytes).map_err(|_| Errno::Fault)?;
            return Ok(0);
        }
        crate::graphics::fbdev::FBIOGETCMAP
        | crate::graphics::fbdev::FBIOPUTCMAP
        | crate::graphics::fbdev::FBIOBLANK => {
            return Ok(0);
        }
        crate::graphics::fbdev::FBIOGET_FSCREENINFO => {
            let (fb_start, fb_end) =
                crate::graphics::framebuffer::framebuffer_range().ok_or(Errno::Invalid)?;
            let writer = crate::graphics::framebuffer::WRITER.lock();
            let Some(ref fb) = writer.framebuffer else {
                return Err(Errno::Invalid);
            };
            let fix = crate::graphics::fbdev::FbFixScreeninfo {
                smem_start: fb_start,
                smem_len: (fb_end - fb_start) as u32,
                line_length: fb.pitch,
                ..Default::default()
            };
            drop(writer);

            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &fix as *const _ as *const u8,
                    core::mem::size_of::<crate::graphics::fbdev::FbFixScreeninfo>(),
                )
            };
            entry::copy_to_user(arg, bytes).map_err(|_| Errno::Fault)?;
            return Ok(0);
        }
        _ => {}
    }

    match request {
        0x5600 => {
            // VT_OPENQRY
            let vt = 2i32;
            entry::copy_to_user(arg, &vt.to_le_bytes()).map_err(|_| Errno::Fault)?;
            return Ok(0);
        }
        0x5601 => {
            // VT_GETMODE
            let mode = [0u8; 8];
            entry::copy_to_user(arg, &mode).map_err(|_| Errno::Fault)?;
            return Ok(0);
        }
        0x5602 => {
            // VT_SETMODE
            return Ok(0);
        }
        0x5603 => {
            // VT_GETSTATE
            let state = [1u16, 0u16, 3u16];
            let bytes = unsafe { core::slice::from_raw_parts(state.as_ptr() as *const u8, 6) };
            entry::copy_to_user(arg, bytes).map_err(|_| Errno::Fault)?;
            return Ok(0);
        }
        0x5605 | 0x5606 | 0x5607 | 0x5608 => return Ok(0), // VT_RELDISP, VT_ACTIVATE, VT_WAITACTIVE, VT_DISALLOCATE
        0x4B3A | 0x4B3B | 0x4B44 | 0x4B45 | 0x4B32 | 0x4B30 => return Ok(0), // KDSETMODE, KDGETMODE, KDGKBMODE, KDSKBMODE, KDSETLED, KDMKTONE
        0x4B46 => return Err(Errno::Invalid),                                // KDGKBENT
        0x8912 => {
            // SIOCGIFCONF
            let zero = 0i32;
            entry::copy_to_user(arg, &zero.to_le_bytes()).map_err(|_| Errno::Fault)?;
            return Ok(0);
        }
        _ => {}
    }

    let (is_tty, is_vt2) = process::with_current_fds(|fds| {
        fds.get(_fd).ok().map(|f| {
            let id = f.inode.lock().id;
            (id == 1 || id == 7, id == 7)
        })
    })
    .flatten()
    .unwrap_or((false, false));

    match request {
        TCGETS => {
            if !is_tty {
                return Err(Errno::NotTty);
            }
            let termios = if is_vt2 {
                crate::tty::VT2_TTY.lock().termios
            } else {
                GLOBAL_TTY.lock().termios
            };
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
            if !is_tty {
                return Err(Errno::NotTty);
            }
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
            if is_vt2 {
                crate::tty::VT2_TTY.lock().termios = termios;
            } else {
                GLOBAL_TTY.lock().termios = termios;
            }
            Ok(0)
        }
        TIOCGWINSZ => {
            if !is_tty {
                return Err(Errno::NotTty);
            }
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
            let pgid = GLOBAL_TTY
                .lock()
                .foreground_pgrp
                .map(|p| p.as_u64() as i32)
                .unwrap_or(0);
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

fn sys_wait4(pid: i64, wstatus: u64, options: u64, _rusage: u64) -> SyscallResult {
    let child = process::wait4(pid, options).map_err(|_| Errno::Child)?;

    match child {
        Some((child_pid, code)) => {
            if wstatus != 0 {
                let status = if code >= 128 && code < 256 {
                    (code - 128) & 0x7f
                } else {
                    (code & 0xff) << 8
                };
                if entry::copy_to_user(wstatus, &status.to_le_bytes()).is_err() {
                    return Err(Errno::Fault);
                }
            }
            Ok(child_pid.as_u64())
        }
        None => Ok(0),
    }
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
            Err("broken pipe") => return Err(Errno::Pipe),
            Err(_) => return Err(Errno::Invalid),
        }
        if scheduler::is_reschedule_requested() {
            scheduler::yield_now();
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

fn sys_poll(fds: u64, nfds: u64, timeout: u64) -> SyscallResult {
    let timeout_ms = timeout as i32;
    let start_ticks = crate::arch::interrupts::vectors::ticks();
    let timeout_ticks = if timeout_ms < 0 {
        u64::MAX
    } else {
        timeout_ms as u64
    };
    if nfds == 0 {
        if timeout_ms > 0 {
            while ticks().saturating_sub(start_ticks) < timeout_ticks {
                if has_pending_signals() {
                    return Err(Errno::Intr);
                }
                sleep(1);
            }
        } else if timeout_ms < 0 {
            loop {
                if has_pending_signals() {
                    return Err(Errno::Intr);
                }
                sleep(1);
            }
        }
        return Ok(0);
    }
    if nfds > 1024 {
        return Err(Errno::Invalid);
    }
    loop {
        let tty_has_input = !crate::tty::GLOBAL_TTY.lock().queue.is_empty();
        let mut ready = 0u64;

        for i in 0..nfds {
            let slot = fds.checked_add(i * 8).ok_or(Errno::Fault)?;
            let mut pfd_buf = [0u8; 8];
            entry::copy_from_user(&mut pfd_buf, slot).map_err(|_| Errno::Fault)?;

            let fd = i32::from_le_bytes(pfd_buf[0..4].try_into().unwrap());
            let events = i16::from_le_bytes(pfd_buf[4..6].try_into().unwrap());
            let mut revents = 0i16;

            if fd >= 0 {
                if fd == 0 || fd == 1 || fd == 2 {
                    if tty_has_input && (events & 1 != 0) {
                        revents |= 1; // POLLIN
                    }
                    if events & 4 != 0 {
                        revents |= 4; // POLLOUT
                    }
                } else {
                    let file = process::with_current_fds(|fds| fds.get(fd as u64).cloned());
                    if let Some(Ok(file)) = file {
                        let (file_ops, inode_id) = {
                            let inode = file.inode.lock();
                            (inode.file_ops, inode.id)
                        };
                        if core::ptr::addr_eq(file_ops, &crate::input::event::EVENT_FILE_OPS)
                            || inode_id == 8
                        {
                            // /dev/input/event0
                            let has_events = !crate::input::event::EVENT_QUEUE.lock().is_empty();
                            if has_events && (events & 1 != 0) {
                                revents |= 1;
                            }
                        } else if core::ptr::addr_eq(file_ops, &crate::input::mouse::MOUSE_FILE_OPS)
                            || inode_id == 8
                        {
                            let has_mouse = !crate::input::mouse::MOUSE_QUEUE.lock().is_empty();
                            if has_mouse && (events & 1 != 0) {
                                revents |= 1;
                            }
                        } else if core::ptr::addr_eq(file_ops, &crate::net::vfs::SOCKET_FILE_OPS) {
                            if let Some(sock) =
                                crate::net::vfs::SOCKET_MAP.lock().get(&inode_id).cloned()
                            {
                                // UNIX domain socket
                                let s = sock.lock();
                                match &s.state {
                                    crate::net::unix::SocketState::Listening {
                                        incoming, ..
                                    } => {
                                        if !incoming.is_empty() && (events & 1 != 0) {
                                            revents |= 1; // Incoming connection to accept
                                        }
                                    }
                                    crate::net::unix::SocketState::Connected {
                                        rx_buffer,
                                        peer_closed,
                                        ..
                                    } => {
                                        if !rx_buffer.is_empty() && (events & 1 != 0) {
                                            revents |= 1; // Data ready to read
                                        }
                                        if *peer_closed {
                                            revents |= 1 | 0x10; // EOF / POLLHUP
                                        }
                                        if !*peer_closed && (events & 4 != 0) {
                                            revents |= 4; // Socket is writable
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        } else {
                            revents |= events & 5;
                        }
                    } else {
                        revents |= 0x20;
                    }
                }
            }

            if revents != 0 {
                ready += 1;
            }

            entry::copy_to_user(slot + 6, &revents.to_le_bytes()).map_err(|_| Errno::Fault)?;
        }

        if ready > 0 || timeout_ms == 0 {
            return Ok(ready);
        }

        if crate::task::process::has_pending_signals() {
            return Err(Errno::Intr);
        }

        let elapsed = crate::arch::interrupts::vectors::ticks().saturating_sub(start_ticks);
        if elapsed >= timeout_ticks {
            return Ok(0);
        }

        scheduler::sleep(1);
    }
}

fn check_fd_ready(fd: u64, for_write: bool) -> bool {
    let file = process::with_current_fds(|fds| fds.get(fd).cloned());
    if let Some(Ok(file)) = file {
        let (file_ops, inode_id, file_type) = {
            let inode = file.inode.lock();
            (inode.file_ops, inode.id, inode.file_type)
        };
        if core::ptr::addr_eq(file_ops, &crate::tty::vfs::TTY_FILE_OPS) || inode_id == 1 {
            // TTY (Console / Shell)
            if for_write {
                return true;
            }
            return !crate::tty::GLOBAL_TTY.lock().queue.is_empty();
        }
        if core::ptr::addr_eq(file_ops, &crate::tty::vfs::VT2_FILE_OPS) || inode_id == 7 {
            // VT2 (X11 Virtual Console)
            if for_write {
                return true;
            }
            return !crate::tty::VT2_TTY.lock().queue.is_empty();
        }
        if core::ptr::addr_eq(file_ops, &crate::input::event::EVENT_FILE_OPS) || inode_id == 8 {
            // /dev/input/event0
            return !for_write && !crate::input::event::EVENT_QUEUE.lock().is_empty();
        }
        if core::ptr::addr_eq(file_ops, &crate::input::mouse::MOUSE_FILE_OPS) || inode_id == 8 {
            // /dev/mouse, /dev/input/mice, /dev/psaux
            if for_write {
                return true;
            }
            return !crate::input::mouse::MOUSE_QUEUE.lock().is_empty();
        }
        if core::ptr::addr_eq(file_ops, &crate::net::vfs::SOCKET_FILE_OPS) {
            if let Some(sock) = crate::net::vfs::SOCKET_MAP.lock().get(&inode_id).cloned() {
                let s = sock.lock();
                match &s.state {
                    crate::net::unix::SocketState::Listening { incoming, .. } => {
                        return !for_write && !incoming.is_empty();
                    }
                    crate::net::unix::SocketState::Connected {
                        rx_buffer,
                        peer_closed,
                        ..
                    } => {
                        if for_write {
                            return !*peer_closed;
                        } else {
                            return !rx_buffer.is_empty() || *peer_closed;
                        }
                    }
                    _ => {}
                }
                return false;
            }
        }
        if file_type == crate::storage::vfs::inode::FileType::Device {
            return for_write;
        }
        return true;
    }
    false
}

fn sys_select(
    nfds: u64,
    readfds: u64,
    writefds: u64,
    _exceptfds: u64,
    timeout: u64,
) -> SyscallResult {
    if nfds > 1024 {
        return Err(Errno::Invalid);
    }
    let num_words = (nfds as usize + 63) / 64;

    let mut in_read = [0u64; 16];
    let mut in_write = [0u64; 16];
    if readfds != 0 && num_words > 0 {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(in_read.as_mut_ptr() as *mut u8, num_words * 8)
        };
        entry::copy_from_user(bytes, readfds).map_err(|_| Errno::Fault)?;
    }
    if writefds != 0 && num_words > 0 {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(in_write.as_mut_ptr() as *mut u8, num_words * 8)
        };
        entry::copy_from_user(bytes, writefds).map_err(|_| Errno::Fault)?;
    }

    let deadline = if timeout != 0 {
        let mut tv_bytes = [0u8; 16];
        if entry::copy_from_user(&mut tv_bytes, timeout).is_ok() {
            let sec = i64::from_le_bytes(tv_bytes[0..8].try_into().unwrap());
            let sub = i64::from_le_bytes(tv_bytes[8..16].try_into().unwrap());
            if sec < 0 || sub < 0 {
                return Err(Errno::Invalid);
            }
            let ms = (sec as u64)
                .saturating_mul(1000)
                .saturating_add(if sub > 1_000_000 {
                    (sub as u64) / 1_000_000
                } else {
                    (sub as u64) / 1000
                });
            Some(crate::arch::interrupts::vectors::ticks().saturating_add(ms))
        } else {
            None
        }
    } else {
        None
    };

    loop {
        let mut out_read = [0u64; 16];
        let mut out_write = [0u64; 16];
        let mut count = 0u64;

        for fd in 0..nfds {
            let w = (fd / 64) as usize;
            let bit = 1u64 << (fd % 64);

            if readfds != 0 && (in_read[w] & bit != 0) {
                if check_fd_ready(fd, false) {
                    out_read[w] |= bit;
                    count += 1;
                }
            }

            if writefds != 0 && (in_write[w] & bit != 0) {
                if check_fd_ready(fd, true) {
                    out_write[w] |= bit;
                    count += 1;
                }
            }
        }

        if count > 0 {
            if readfds != 0 && num_words > 0 {
                let bytes = unsafe {
                    core::slice::from_raw_parts(out_read.as_ptr() as *const u8, num_words * 8)
                };
                entry::copy_to_user(readfds, bytes).map_err(|_| Errno::Fault)?;
            }
            if writefds != 0 && num_words > 0 {
                let bytes = unsafe {
                    core::slice::from_raw_parts(out_write.as_ptr() as *const u8, num_words * 8)
                };
                entry::copy_to_user(writefds, bytes).map_err(|_| Errno::Fault)?;
            }
            return Ok(count);
        }

        if let Some(dl) = deadline {
            if crate::arch::interrupts::vectors::ticks() >= dl {
                if readfds != 0 && num_words > 0 {
                    let bytes = unsafe {
                        core::slice::from_raw_parts(out_read.as_ptr() as *const u8, num_words * 8)
                    };
                    entry::copy_to_user(readfds, bytes).map_err(|_| Errno::Fault)?;
                }
                if writefds != 0 && num_words > 0 {
                    let bytes = unsafe {
                        core::slice::from_raw_parts(out_write.as_ptr() as *const u8, num_words * 8)
                    };
                    entry::copy_to_user(writefds, bytes).map_err(|_| Errno::Fault)?;
                }
                return Ok(0);
            }
        }

        if crate::task::process::has_pending_signals() {
            return Err(Errno::Intr);
        }

        scheduler::yield_or_wait();
    }
}

fn sys_read(fd: u64, buf: u64, len: u64) -> SyscallResult {
    if len == 0 {
        return Ok(0);
    }

    let file = process::with_current_fds(|fds| fds.get(fd).cloned());
    let Some(Ok(mut file)) = file else {
        return Err(Errno::BadFd);
    };

    let is_dev = file.inode.lock().file_type == crate::storage::vfs::inode::FileType::Device;
    if is_dev {
        let (file_ops, inode_id) = {
            let inode = file.inode.lock();
            (inode.file_ops, inode.id)
        };
        if file.flags.nonblock {
            if (core::ptr::addr_eq(file_ops, &crate::tty::vfs::TTY_FILE_OPS) || inode_id == 1)
                && crate::tty::GLOBAL_TTY.lock().queue.is_empty()
            {
                return Err(Errno::Again);
            }
            if (core::ptr::addr_eq(file_ops, &crate::tty::vfs::VT2_FILE_OPS) || inode_id == 7)
                && crate::tty::VT2_TTY.lock().queue.is_empty()
            {
                return Err(Errno::Again);
            }
            if (core::ptr::addr_eq(file_ops, &crate::input::event::EVENT_FILE_OPS) || inode_id == 6)
                && crate::input::event::EVENT_QUEUE.lock().is_empty()
            {
                return Err(Errno::Again);
            }
            if (core::ptr::addr_eq(file_ops, &crate::input::mouse::MOUSE_FILE_OPS) || inode_id == 8)
                && crate::input::mouse::MOUSE_QUEUE.lock().is_empty()
            {
                return Err(Errno::Again);
            }
            if core::ptr::addr_eq(file_ops, &crate::net::vfs::SOCKET_FILE_OPS) {
                if let Some(sock) = crate::net::vfs::SOCKET_MAP.lock().get(&inode_id).cloned() {
                    let s = sock.lock();
                    match &s.state {
                        crate::net::unix::SocketState::Connected {
                            rx_buffer,
                            peer_closed,
                            ..
                        } => {
                            if rx_buffer.is_empty() && !*peer_closed {
                                return Err(Errno::Again);
                            }
                        }
                        crate::net::unix::SocketState::Listening { .. } => {
                            return Err(Errno::Invalid);
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut staging = [0u8; 4096];
        let to_read = (len as usize).min(staging.len());
        let read = match file.read(&mut staging[..to_read]) {
            Ok(read) => read,
            Err("interrupted") => return Err(Errno::Intr),
            Err("would block") => return Err(Errno::Again),
            Err(_) => return Err(Errno::Invalid),
        };
        if read > 0 && entry::copy_to_user(buf, &staging[..read]).is_err() {
            return Err(Errno::Fault);
        }
        return Ok(read as u64);
    }

    let mut total_read = 0;
    let target = len as usize;
    let mut staging = [0u8; 4096];

    while total_read < target {
        let chunk = core::cmp::min(target - total_read, 4096);
        let curr_buf = buf + total_read as u64;

        let Some(result) = process::with_current_fds(|fds| fds.read(fd, &mut staging[..chunk]))
        else {
            return Err(Errno::BadFd);
        };

        match result {
            Ok(0) => break,
            Ok(n) => {
                if entry::copy_to_user(curr_buf, &staging[..n]).is_err() {
                    return Err(Errno::Fault);
                }
                total_read += n;
                if n < chunk {
                    break;
                }
            }
            Err("not a regular or device file") => return Err(Errno::IsDir),
            Err(_) => return Err(Errno::Invalid),
        }
        if scheduler::is_reschedule_requested() {
            scheduler::yield_now();
        }
    }

    Ok(total_read as u64)
}
fn resolve_relative_path(path: &[u8]) -> Vec<u8> {
    if path.starts_with(b"/") {
        return path.to_vec();
    }
    let cwd = process::current()
        .map(|p| p.lock().cwd.clone())
        .unwrap_or_else(|| String::from("/"));
    let mut p = Vec::new();
    if !cwd.ends_with('/') {
        p.extend_from_slice(cwd.as_bytes());
        p.push(b'/');
    } else {
        p.extend_from_slice(cwd.as_bytes());
    }
    p.extend_from_slice(path);
    p
}

fn sys_open(path: u64, flags: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };
    let path = resolve_relative_path(&path);

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
    let nonblock = (flags & 0x800) != 0;
    let exclusive = (flags & 0x80) != 0;

    let open_flags = OpenFlags {
        read,
        write,
        create,
        truncate,
        append,
        directory,
        nonblock,
        exclusive,
    };

    let Some(result) = process::with_current_fds(|fds| fds.open(&path, open_flags)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(fd) => Ok(fd),
        Err("file already exists") => Err(Errno::Exist),
        Err("not a directory") => Err(Errno::NotDir),
        Err("is a directory") => Err(Errno::IsDir),
        Err(_) => Err(Errno::NoEntry),
    }
}

fn sys_close(fd: u64) -> SyscallResult {
    let file = process::with_current_fds(|fds| fds.get(fd).cloned().ok()).flatten();

    let Some(result) = process::with_current_fds(|fds| fds.close(fd)) else {
        return Err(Errno::BadFd);
    };

    result.map_err(|_| Errno::BadFd)?;

    if let Some(f) = file {
        let is_socket = {
            let inode = f.inode.lock();
            core::ptr::addr_eq(inode.file_ops, &crate::net::vfs::SOCKET_FILE_OPS)
        };

        if is_socket && alloc::sync::Arc::strong_count(&f.inode) <= 1 {
            let id = f.inode.lock().id;
            if let Some(sock) = crate::net::vfs::SOCKET_MAP.lock().remove(&id) {
                let peer_opt = {
                    let mut s = sock.lock();
                    let peer =
                        if let crate::net::unix::SocketState::Connected { peer, .. } = &s.state {
                            peer.upgrade()
                        } else {
                            None
                        };
                    s.state = crate::net::unix::SocketState::Closed;
                    peer
                };
                if let Some(peer_arc) = peer_opt {
                    if !alloc::sync::Arc::ptr_eq(&sock, &peer_arc) {
                        let mut p = peer_arc.lock();
                        if let crate::net::unix::SocketState::Connected { peer_closed, .. } =
                            &mut p.state
                        {
                            *peer_closed = true;
                        }
                    }
                }
            }
        }
    }

    Ok(0)
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
    if addr >= USER_END {
        return Err(Errno::Fault);
    }
    x86_64::registers::model_specific::FsBase::write(VirtAddr::new(addr));

    Ok(0)
}

fn sys_mkdir(path: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };
    let path = resolve_relative_path(&path);

    if path == b"/tmp/.X11-unix" || path == b"/tmp" {
        return Ok(0);
    }

    let Some(result) = process::with_current_fds(|fds| fds.mkdir(&path)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(()) => Ok(0),
        Err("file already exists") => Err(Errno::Exist),
        Err("not a directory") => Err(Errno::NotDir),
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
    let path = resolve_relative_path(&path);

    let mut reg = SOCKET_REGISTRY.lock();
    if reg.remove(&path).is_some() {
        return Ok(0);
    }
    drop(reg);

    if path.starts_with(b"/tmp/.X11-unix") {
        return Ok(0);
    }

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
    let path = resolve_relative_path(&path);

    let Some(result) = process::with_current_fds(|fds| fds.rmdir(&path)) else {
        return Err(Errno::BadFd);
    };

    match result {
        Ok(()) => Ok(0),
        Err("directory not empty") => Err(Errno::NotEmpty),
        Err("not a directory") => Err(Errno::NotDir),
        Err(_) => Err(Errno::NoEntry),
    }
}

fn sys_create(path: u64) -> SyscallResult {
    let Ok(path) = entry::user_path(path) else {
        return Err(Errno::Fault);
    };
    let path = resolve_relative_path(&path);

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
    if pid == 0 {
        if crate::task::process::send_signal(crate::task::identity::current_pid(), sig as usize) {
            return Ok(0);
        } else {
            return Err(Errno::Srch);
        }
    }
    if pid < 0 {
        if crate::task::process::send_signal_pgrp(
            crate::task::identity::ProcessId((-pid) as u64),
            sig as usize,
        ) {
            return Ok(0);
        } else {
            return Err(Errno::Srch);
        }
    }
    if crate::task::process::send_signal(crate::task::identity::ProcessId(pid as u64), sig as usize)
    {
        Ok(0)
    } else {
        Err(Errno::Srch)
    }
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

    if ctx.rip >= crate::memory::mmu::USER_END || ctx.rsp >= crate::memory::mmu::USER_END {
        return Err(Errno::Fault);
    }

    frame.regs = ctx.regs;
    frame.rip = ctx.rip;
    frame.rsp = ctx.rsp;
    const SAFE_RFLAGS_MASK: u64 = 0x00244dd5;
    frame.rflags = (ctx.rflags & SAFE_RFLAGS_MASK) | 0x202;
    frame.cs = u64::from(crate::arch::gdt::user_code_selector().0) | 0x3;
    frame.ss = u64::from(crate::arch::gdt::user_data_selector().0) | 0x3;

    if let Some(proc) = process::current() {
        const UNBLOCKABLE: u64 = (1u64 << (9 - 1)) | (1u64 << (19 - 1));
        proc.lock().blocked_signals = ctx.mask & !UNBLOCKABLE;
    }

    Ok(frame.regs.rax)
}

fn parse_sun_path(raw: &[u8]) -> alloc::vec::Vec<u8> {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    raw[..end].to_vec()
}

fn get_socket_by_fd(
    fd: u64,
) -> Result<alloc::sync::Arc<spin::Mutex<crate::net::unix::UnixSocket>>, Errno> {
    let file = process::with_current_fds(|fds| fds.get(fd).cloned()).ok_or(Errno::BadFd)?;
    let f = file.map_err(|_| Errno::BadFd)?;
    let inode = f.inode.lock();
    if !core::ptr::addr_eq(inode.file_ops, &crate::net::vfs::SOCKET_FILE_OPS) {
        return Err(Errno::BadFd);
    }
    let inode_id = inode.id;
    let map = crate::net::vfs::SOCKET_MAP.lock();
    map.get(&inode_id).cloned().ok_or(Errno::BadFd)
}
