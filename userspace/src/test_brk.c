#include "syscall.h"

int main(void) {
    char *base = (char *)sys_brk(0);
    if (!base) {
        sys_write(1, "brk(0) failed\n", 14);
        return 1;
    }

    char *expanded = (char *)sys_brk(base + 8192);
    if (expanded <= base) {
        sys_write(1, "expanding brk failed\n", 21);
        return 1;
    }

    base[0] = 'H';
    base[1] = 'E';
    base[2] = 'A';
    base[3] = 'P';
    base[4] = ' ';
    base[5] = 'O';
    base[6] = 'K';
    base[7] = '\n';

    base[4096] = '!';

    sys_write(1, base, 8);
    return 0;
}