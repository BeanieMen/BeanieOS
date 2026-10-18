#include "syscall.h"

int main(void) {
    // 1. Query current break
    char *base = (char *)sys_brk(0);
    if (!base) {
        sys_write(1, "brk(0) failed\n", 14);
        return 1;
    }

    // 2. Expand heap by 8 KiB (2 pages)
    char *expanded = (char *)sys_brk(base + 8192);
    if (expanded <= base) {
        sys_write(1, "expanding brk failed\n", 21);
        return 1;
    }

    // 3. Write and verify memory on both page 1 and page 2
    base[0] = 'H';
    base[1] = 'E';
    base[2] = 'A';
    base[3] = 'P';
    base[4] = ' ';
    base[5] = 'O';
    base[6] = 'K';
    base[7] = '\n';

    base[4096] = '!'; // second 4KB page

    sys_write(1, base, 8);
    return 0;
}