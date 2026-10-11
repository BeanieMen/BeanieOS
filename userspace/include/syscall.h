#ifndef SYSCALL_H
#define SYSCALL_H

#define SYS_EXIT    0
#define SYS_WRITE   1
#define SYS_READ    2
#define SYS_OPEN    3
#define SYS_CLOSE   4
#define SYS_DIR     3
#define SYS_MKDIR   5
#define SYS_GETDENTS 6
#define SYS_YIELD   7
#define SYS_GETPID  8
#define SYS_UNLINK  9
#define SYS_RMDIR   10
#define SYS_CREATE  11

static inline long syscall(long num, long a1, long a2, long a3) {
    long ret;
    __asm__ volatile (
        "int $0x80"
        : "=a" (ret)
        : "a" (num), "D" (a1), "S" (a2), "d" (a3)
        : "memory"
    );
    return ret;
}

static inline void sys_exit(int code) {
    syscall(SYS_EXIT, code, 0, 0);
    while (1) {}
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

static inline long sys_yield(void) {
    return syscall(SYS_YIELD, 0, 0, 0);
}

static inline long sys_open_dir(const char *path) {
    return syscall(SYS_DIR, (long)path, 3, 0);
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