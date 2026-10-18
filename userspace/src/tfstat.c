#include "syscall.h"

// Linux x86_64 struct stat layout excerpt
struct stat {
    char pad[48];
    unsigned long st_size; // offset 48
    char rest[88];
};

int main(void) {
    // 1. Open an existing file on disk
    long fd = sys_open("/INIT", 0 /* O_RDONLY */);
    if (fd < 0) {
        sys_write(1, "open failed\n", 12);
        return 1;
    }

    // 2. Query file statistics via sys_fstat
    struct stat st;
    long ret = syscall(SYS_FSTAT, fd, (long)&st, 0);
    sys_close(fd);

    if (ret != 0) {
        sys_write(1, "fstat failed\n", 13);
        return 2;
    }

    // 3. Verify size is positive (> 0)
    if (st.st_size == 0) {
        sys_write(1, "size is zero\n", 13);
        return 3;
    }

    sys_write(1, "FSTAT OK\n", 9);
    return 0;
}