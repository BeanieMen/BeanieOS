#include "syscall.h"

int main(void) {
    long ret = syscall(16 /* SYS_IOCTL */, 1, 0x5413 /* TIOCGWINSZ */, 0);
    if (ret != -25) {
        sys_write(1, "ioctl failed\n", 13);
        return 1;
    }

    long tid = syscall(218 /* SYS_SET_TID_ADDRESS */, 0, 0, 0);
    if (tid <= 0) {
        sys_write(1, "set_tid_address failed\n", 23);
        return 2;
    }

    sys_write(1, "STUBS OK\n", 9);

    syscall(231 /* SYS_EXIT_GROUP */, 0, 0, 0);

    return 3;
}