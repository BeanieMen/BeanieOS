#include "syscall.h"

int main(void) {
    const char msg[] = "hi\n";
    sys_write(1, msg, sizeof(msg) - 1);
    sys_exit(0);

    return 0;
}