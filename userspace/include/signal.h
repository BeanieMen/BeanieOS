#ifndef _SIGNAL_H
#define _SIGNAL_H

#include "syscall.h"

#define SIGHUP   1
#define SIGINT   2
#define SIGQUIT  3
#define SIGKILL  9
#define SIGSEGV  11
#define SIGTERM  15

#define SA_RESTORER 0x04000000

typedef void (*sighandler_t)(int);

struct sigaction {
    sighandler_t sa_handler;
    unsigned long sa_flags;
    void (*sa_restorer)(void);
    unsigned long sa_mask;
};

// Trampoline that invokes sys_rt_sigreturn (Syscall 15) when the handler returns
static inline void __restore_rt(void) {
    __asm__ volatile (
        "mov $15, %%rax\n\t"
        "syscall\n\t"
        : : : "rax"
    );
}

static inline int sigaction(int signum, const struct sigaction *act, struct sigaction *oldact) {
    struct sigaction a;
    if (act) {
        a = *act;
        a.sa_flags |= SA_RESTORER;
        a.sa_restorer = __restore_rt;
        act = &a;
    }
    return sys_rt_sigaction(signum, act, oldact, 8);
}

#endif