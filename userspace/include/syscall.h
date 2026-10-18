#ifndef SYSCALL_H
#define SYSCALL_H

// Linux x86_64 syscall numbers
#define SYS_READ     0
#define SYS_WRITE    1
#define SYS_OPEN     2
#define SYS_CLOSE    3
#define SYS_LSEEK    8
#define SYS_BRK      12
#define SYS_YIELD    24
#define SYS_GETPID   39
#define SYS_FORK     57
#define SYS_EXECVE   59
#define SYS_EXIT     60
#define SYS_WAIT4    61
#define SYS_GETDENTS 217
#define SYS_POLL 7
#define POLLIN   1

#define SYS_MKDIR    83
#define SYS_RMDIR    84
#define SYS_CREATE   85
#define SYS_UNLINK   87

/* lseek whence */
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

/* open flags */
#define O_RDONLY    0
#define O_WRONLY    1
#define O_RDWR      2
#define O_CREAT     0100
#define O_TRUNC     01000
#define O_APPEND    02000
#define O_DIRECTORY 0200000
#define O_CLOEXEC   02000000

/* errno values. The kernel returns these negated, so a negative return means
 * failure and errno is the negation. */
#define EPERM   1
#define ENOENT  2
#define EBADF   9
#define ENOMEM  12
#define EFAULT  14
#define ENOTDIR 20
#define EISDIR  21
#define EINVAL  22
#define EMFILE  24
#define ENOSYS  38

static inline long syscall(long num, long a1, long a2, long a3) {
    long ret;
    __asm__ volatile (
        "int $0x80"
        : "=a" (ret)
        : "a" (num), "D" (a1), "S" (a2), "d" (a3)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static inline void *sys_brk(void *addr) {
    return (void *)syscall(SYS_BRK, (long)addr, 0, 0);
}

static inline long sys_poll(void *fds, unsigned long nfds, unsigned short events) {
    return syscall(SYS_POLL, (long)fds, nfds, events);
}

static inline long sys_fork(void) {
    return syscall(SYS_FORK, 0, 0, 0);
}

static inline long sys_wait4(long pid, int *wstatus, int options, void *rusage) {
    (void)rusage;
    return syscall(SYS_WAIT4, pid, (long)wstatus, options);
}

static inline void sys_exit(int code) {
    syscall(SYS_EXIT, code, 0, 0);
    while (1) {}
}
static inline long sys_execve(const char *path, char *const argv[], char *const envp[]){
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

static inline long sys_close(int fd) {
    return syscall(SYS_CLOSE, fd, 0, 0);
}

static inline long sys_lseek(int fd, long offset, int whence) {
    return syscall(SYS_LSEEK, fd, offset, whence);
}
static inline long sys_yield(void) {
    return syscall(SYS_YIELD, 0, 0, 0);
}

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

static inline long sys_getpid(void) {
    return syscall(SYS_GETPID, 0, 0, 0);
}

#endif