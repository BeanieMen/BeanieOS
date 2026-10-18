#include "syscall.h"

static unsigned long strlen(const char *s)
{
    unsigned long n = 0;

    while (s[n]) {
        n++;
    }

    return n;
}

int main(int argc, char **argv) {
    for (int i=0; i < argc; i++) {
        sys_write(1, argv[i], strlen(argv[i]));
        sys_write(1, "\n", 1);
    }
    return 0;
}