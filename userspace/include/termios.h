#ifndef TERMIOS_H
#define TERMIOS_H

#include "syscall.h"

#define TCGETS      0x5401
#define TCSETS      0x5402
#define TCSETSW     0x5403
#define TCSETSF     0x5404
#define TIOCGWINSZ  0x5413

#define ICANON 0000002
#define ECHO   0000010
#define ISIG   0000001

#define TCSANOW 0

struct termios {
    unsigned int c_iflag;
    unsigned int c_oflag;
    unsigned int c_cflag;
    unsigned int c_lflag;
    unsigned char c_line;
    unsigned char c_cc[32];
};

struct winsize {
    unsigned short ws_row;
    unsigned short ws_col;
    unsigned short ws_xpixel;
    unsigned short ws_ypixel;
};

static inline int tcgetattr(int fd, struct termios *termios_p) {
    return sys_ioctl(fd, TCGETS, termios_p);
}

static inline int tcsetattr(int fd, int optional_actions, const struct termios *termios_p) {
    (void)optional_actions;
    return sys_ioctl(fd, TCSETS, (void *)termios_p);
}

static inline int isatty(int fd) {
    struct termios t;
    return sys_ioctl(fd, TCGETS, &t) == 0;
}

static inline void set_raw_mode(int enable) {
    static struct termios saved_termios;
    static int has_saved = 0;

    if (enable) {
        if (tcgetattr(0, &saved_termios) == 0) {
            has_saved = 1;
            struct termios t = saved_termios;
            t.c_lflag &= ~(ICANON | ECHO);
            tcsetattr(0, TCSANOW, &t);
        }
    } else {
        if (has_saved) {
            tcsetattr(0, TCSANOW, &saved_termios);
        } else {
            struct termios t;
            if (tcgetattr(0, &t) == 0) {
                t.c_lflag |= (ICANON | ECHO);
                tcsetattr(0, TCSANOW, &t);
            }
        }
    }
}

#endif