#include "syscall.h"

#define PROT_READ   0x1
#define PROT_WRITE  0x2
#define MAP_PRIVATE 0x02
#define MAP_ANON    0x20

int main(void) {
    // 1. Request 2 pages (8 KiB) via anonymous mmap
    char *buf = (char *)sys_mmap(0, 8192, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
    if ((long)buf <= 0) {
        sys_write(1, "mmap failed\n", 12);
        return 1;
    }

    // 2. Write and read from page 1 and page 2
    buf[0] = 'M';
    buf[1] = 'M';
    buf[2] = 'A';
    buf[3] = 'P';
    buf[4] = ' ';
    buf[5] = 'O';
    buf[6] = 'K';
    buf[7] = '\n';

    buf[4096] = 'X'; // Second page

    sys_write(1, buf, 8);

    // 3. Test munmap
    long unmap_ret = sys_munmap(buf, 8192);
    if (unmap_ret != 0) {
        sys_write(1, "munmap failed\n", 14);
        return 2;
    }

    return 0;
}