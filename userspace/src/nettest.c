#include "syscall.h"

struct sockaddr_in {
    unsigned short sin_family;
    unsigned short sin_port;
    unsigned int sin_addr;
    char sin_zero[8];
};

static unsigned short htons(unsigned short v) {
    return (v << 8) | (v >> 8);
}

int main(void) {
    int fd = sys_socket(2, 1, 0);
    if (fd < 0) {
        const char err[] = "nettest: socket failed\n";
        sys_write(1, err, sizeof(err) - 1);
        sys_exit(1);
    }

    struct sockaddr_in addr;
    addr.sin_family = 2;
    addr.sin_port = htons(80);
    unsigned char ip[4] = {142, 250, 118, 113};
    for (int i = 0; i < 4; i++) {
        ((unsigned char *)&addr.sin_addr)[i] = ip[i];
    }
    for (int i = 0; i < 8; i++) addr.sin_zero[i] = 0;

    const char connecting[] = "Connecting to Google (142.250.118.113:80)...\n";
    sys_write(1, connecting, sizeof(connecting) - 1);

    int ret = sys_connect(fd, (const struct sockaddr_un *)&addr, sizeof(addr));
    if (ret < 0) {
        const char err[] = "nettest: connect failed\n";
        sys_write(1, err, sizeof(err) - 1);
        sys_close(fd);
        sys_exit(1);
    }

    const char msg[] = "GET / HTTP/1.0\r\nHost: www.google.com\r\nUser-Agent: BeanieOS\r\n\r\n";
    sys_write(fd, msg, sizeof(msg) - 1);

    char buf[1024];
    long n;
    while ((n = sys_read(fd, buf, sizeof(buf) - 1)) > 0) {
        sys_write(1, buf, n);
    }

    sys_close(fd);
    sys_exit(0);
    return 0;
}
