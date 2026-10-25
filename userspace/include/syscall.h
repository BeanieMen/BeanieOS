#ifndef SYSCALL_H
#define SYSCALL_H

// Linux x86_64 syscall numbers
#define SYS_READ 0
#define SYS_WRITE 1
#define SYS_OPEN 2
#define SYS_CLOSE 3
#define SYS_FSTAT 5
#define SYS_LSEEK 8
#define SYS_MMAP 9
#define SYS_MUNMAP 11
#define SYS_BRK 12
#define SYS_RT_SIGACTION 13
#define SYS_IOCTL 16
#define SYS_YIELD 24
#define SYS_NANOSLEEP 35
#define SYS_GETPID 39

#define SYS_SOCKET 41
#define SYS_CONNECT 42
#define SYS_ACCEPT 43
#define SYS_BIND 49
#define SYS_LISTEN 50
#define SYS_SOCKETPAIR 53

#define AF_UNIX 1
#define SOCK_STREAM 1

struct sockaddr_un {
  unsigned short sun_family;
  char sun_path[108];
};

#define SYS_FORK 57
#define SYS_EXECVE 59
#define SYS_EXIT 60
#define SYS_WAIT4 61
#define SYS_GETDENTS 217
#define SYS_POLL 7
#define POLLIN 1
#define SYS_CLOCK_GETTIME 228

#define CLOCK_REALTIME 0
#define CLOCK_MONOTONIC 1

#define SYS_MKDIR 83
#define SYS_RMDIR 84
#define SYS_CREATE 85
#define SYS_UNLINK 87

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_CREAT 0100
#define O_TRUNC 01000
#define O_APPEND 02000
#define O_DIRECTORY 0200000
#define O_CLOEXEC 02000000

/* errno values. The kernel returns these negated, so a negative return means
 * failure and errno is the negation. */
#define EPERM 1
#define ENOENT 2
#define EBADF 9
#define ENOMEM 12
#define EFAULT 14
#define ENOTDIR 20
#define EISDIR 21
#define EINVAL 22
#define EMFILE 24
#define ENOSYS 38

#define AF_UNIX 1
#define SOCK_STREAM 1

struct timespec {
  long tv_sec;
  long tv_nsec;
};

struct socketaddr_un {
  unsigned short sun_family;
  char sun_path[100];
};

static inline long syscall(long num, long a1, long a2, long a3) {
  long ret;
  __asm__ volatile("int $0x80"
                   : "=a"(ret)
                   : "a"(num), "D"(a1), "S"(a2), "d"(a3)
                   : "rcx", "r11", "memory");
  return ret;
}

static inline long syscall6(long num, long a1, long a2, long a3, long a4,
                            long a5, long a6) {
  long ret;
  register long r10 __asm__("r10") = a4;
  register long r8 __asm__("r8") = a5;
  register long r9 __asm__("r9") = a6;
  __asm__ volatile("int $0x80"
                   : "=a"(ret)
                   : "a"(num), "D"(a1), "S"(a2), "d"(a3), "r"(r10), "r"(r8),
                     "r"(r9)
                   : "rcx", "r11", "memory");
  return ret;
}

static inline int sys_socket(int domain, int type, int protocol) {
  return (int)syscall(SYS_SOCKET, domain, type, protocol);
}

static inline int sys_bind(int fd, const struct sockaddr_un *addr,
                           int addrlen) {
  return (int)syscall(SYS_BIND, fd, (long)addr, addrlen);
}
static inline int sys_listen(int fd, int backlog) {
  return (int)syscall(SYS_LISTEN, fd, backlog, 0);
}
static inline int sys_connect(int fd, const struct sockaddr_un *addr,
                              int addrlen) {
  return (int)syscall(SYS_CONNECT, fd, (long)addr, addrlen);
}
static inline int sys_accept(int fd, struct sockaddr_un *addr, int *addrlen) {
  return (int)syscall(SYS_ACCEPT, fd, (long)addr, (long)addrlen);
}
static inline int sys_socketpair(int domain, int type, int protocol,
                                 int sv[2]) {
  return (int)syscall6(SYS_SOCKETPAIR, domain, type, protocol, (long)sv, 0, 0);
}

static inline int sys_nanosleep(const struct timespec *req,
                                struct timespec *rem) {
  return (int)syscall(SYS_NANOSLEEP, (long)req, (long)rem, 0);
}

static inline int sys_clock_gettime(int clock_id, struct timespec *tp) {
  return (int)syscall(SYS_CLOCK_GETTIME, clock_id, (long)tp, 0);
}

static inline void sys_sleep_ms(unsigned long ms) {
  struct timespec ts;
  ts.tv_sec = ms / 1000;
  ts.tv_nsec = (ms % 1000) * 1000000;
  sys_nanosleep(&ts, 0);
}
static inline void *sys_mmap(void *addr, unsigned long len, int prot, int flags,
                             int fd, long offset) {
  return (void *)syscall6(SYS_MMAP, (long)addr, len, prot, flags, fd, offset);
}
static inline long sys_ioctl(int fd, unsigned long req, void *arg) {
  return syscall(SYS_IOCTL, fd, (long)req, (long)arg);
}

static inline int sys_rt_sigaction(int sig, const void *act, void *oldact,
                                   unsigned long sigsetsize) {
  return (int)syscall6(SYS_RT_SIGACTION, sig, (long)act, (long)oldact,
                       sigsetsize, 0, 0);
}

static inline long sys_munmap(void *addr, unsigned long len) {
  return (int)syscall(SYS_MUNMAP, (long)addr, len, 0);
}
static inline void *sys_brk(void *addr) {
  return (void *)syscall(SYS_BRK, (long)addr, 0, 0);
}

static inline long sys_poll(void *fds, unsigned long nfds,
                            unsigned short events) {
  return syscall(SYS_POLL, (long)fds, nfds, events);
}

static inline long sys_fork(void) { return syscall(SYS_FORK, 0, 0, 0); }

static inline long sys_wait4(long pid, int *wstatus, int options,
                             void *rusage) {
  (void)rusage;
  return syscall(SYS_WAIT4, pid, (long)wstatus, options);
}

static inline void sys_exit(int code) {
  syscall(SYS_EXIT, code, 0, 0);
  while (1) {
  }
}
static inline long sys_execve(const char *path, char *const argv[],
                              char *const envp[]) {
  return syscall(SYS_EXECVE, (long)path, (long)argv, (long)envp);
}

static inline long sys_write(int fd, const void *buf, unsigned long len) {
  return syscall(SYS_WRITE, fd, (long)buf, (long)len);
}

static inline long sys_read(int fd, void *buf, unsigned long len) {
  return syscall(SYS_READ, fd, (long)buf, (long)len);
}

static inline long sys_open(const char *path, int flags) {
  return syscall(SYS_OPEN, (long)path, flags, 0);
}

static inline long sys_close(int fd) { return syscall(SYS_CLOSE, fd, 0, 0); }

static inline long sys_lseek(int fd, long offset, int whence) {
  return syscall(SYS_LSEEK, fd, offset, whence);
}
static inline long sys_yield(void) { return syscall(SYS_YIELD, 0, 0, 0); }

static inline long sys_open_dir(const char *path) {
  return sys_open(path, O_RDONLY | O_DIRECTORY);
}

static inline long sys_getdents(long fd, void *buf, unsigned long len) {
  return syscall(SYS_GETDENTS, fd, (long)buf, (long)len);
}

static inline long sys_mkdir(const char *path) {
  return syscall(SYS_MKDIR, (long)path, 0, 0);
}

static inline long sys_unlink(const char *path) {
  return syscall(SYS_UNLINK, (long)path, 0, 0);
}

static inline long sys_rmdir(const char *path) {
  return syscall(SYS_RMDIR, (long)path, 0, 0);
}

static inline long sys_create(const char *path) {
  return syscall(SYS_CREATE, (long)path, 0, 0);
}

static inline long sys_getpid(void) { return syscall(SYS_GETPID, 0, 0, 0); }

#endif
