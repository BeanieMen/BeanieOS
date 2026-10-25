#include "syscall.h"

int main() {
  sys_write(1, "1. Testing socketpair...\n", 25);
  int sv[2];
  if (sys_socketpair(AF_UNIX, SOCK_STREAM, 0, sv) < 0) {
    sys_write(1, "socketpair FAIL\n", 16);
    return 1;
  }

  sys_write(sv[0], "PING", 4);
  char buf[8] = {0};
  sys_read(sv[1], buf, 4);
  if (buf[0] != 'P' || buf[1] != 'I' || buf[2] != 'N' || buf[3] != 'G') {
    sys_write(1, "read FAIL\n", 10);
    return 1;
  }
  sys_write(1, "socketpair PASS\n", 16);

  sys_write(1, "2. Testing bind/listen/connect/accept...\n", 41);
  struct sockaddr_un addr;
  addr.sun_family = AF_UNIX;
  for (int i = 0; i < 108; i++) addr.sun_path[i] = 0;
  addr.sun_path[0] = '/'; addr.sun_path[1] = 't'; addr.sun_path[2] = 0;

  int srv = sys_socket(AF_UNIX, SOCK_STREAM, 0);
  sys_bind(srv, &addr, sizeof(addr));
  sys_listen(srv, 5);

  int pid = sys_fork();
  if (pid == 0) {
    int cli = sys_socket(AF_UNIX, SOCK_STREAM, 0);
    sys_connect(cli, &addr, sizeof(addr));
    sys_write(cli, "HELO", 4);
    char reply[8] = {0};
    sys_read(cli, reply, 4);
    sys_write(1, "Client received: ", 17);
    sys_write(1, reply, 4);
    sys_write(1, "\n", 1);
    sys_close(cli);
    sys_exit(0);
  }

  int conn = sys_accept(srv, 0, 0);
  char msg[8] = {0};
  sys_read(conn, msg, 4);
  if (msg[0] == 'H' && msg[1] == 'E') {
    sys_write(conn, "ACK!", 4);
    sys_write(1, "Server IPC handshake PASS\n", 26);
  }

  sys_close(conn);
  sys_close(srv);
  return 0;
}