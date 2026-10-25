#include "syscall.h"

int main() {
  sys_write(1, "[xhello] Connecting to /tmp/.X11-unix/X0...\n", 44);

  int fd = sys_socket(AF_UNIX, SOCK_STREAM, 0);
  if (fd < 0) {
    sys_write(1, "[xhello] Failed to create socket\n", 33);
    return 1;
  }

  struct sockaddr_un addr;
  addr.sun_family = AF_UNIX;
  const char *path = "/tmp/.X11-unix/X0";
  int i = 0;
  while (path[i] && i < 107) {
    addr.sun_path[i] = path[i];
    i++;
  }
  addr.sun_path[i] = '\0';

  if (sys_connect(fd, &addr, sizeof(addr)) < 0) {
    sys_write(1, "[xhello] Connection to X server failed\n", 39);
    sys_close(fd);
    return 1;
  }

  sys_write(1, "[xhello] Connected! Sending X11 setup request...\n", 49);

  /* X11 connection setup request packet (12 bytes) */
  unsigned char req[12] = {
    'l',    /* byte order: 'l' = least significant byte first */
    0,      /* unused */
    11, 0,  /* major version 11 */
    0, 0,   /* minor version 0 */
    0, 0,   /* authorization protocol name length = 0 */
    0, 0,   /* authorization protocol data length = 0 */
    0, 0    /* unused */
  };

  if (sys_write(fd, req, sizeof(req)) != sizeof(req)) {
    sys_write(1, "[xhello] Failed to send setup request\n", 38);
    sys_close(fd);
    return 1;
  }

  /* Read 8-byte setup reply header */
  unsigned char reply[8] = {0};
  long n = sys_read(fd, reply, sizeof(reply));
  if (n < 8) {
    sys_write(1, "[xhello] Failed to read reply header\n", 37);
    sys_close(fd);
    return 1;
  }

  if (reply[0] == 1) {
    sys_write(1, "[xhello] SUCCESS: X11 server accepted connection!\n", 50);
  } else if (reply[0] == 0) {
    sys_write(1, "[xhello] FAILED: X11 server rejected connection\n", 48);
  } else if (reply[0] == 2) {
    sys_write(1, "[xhello] AUTH: X11 server requested authentication\n", 51);
  } else {
    sys_write(1, "[xhello] Unknown reply status\n", 30);
  }

  sys_close(fd);
  return 0;
}
