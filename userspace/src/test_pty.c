#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

#define TIOCGPTN 0x80045430
#define TIOCSPTLCK 0x40045431

int main(void) {
  int master = open("/dev/ptmx", O_RDWR);
  if (master < 0) {
    printf("FAIL: open /dev/ptmx failed\n");
    return 1;
  }

  int ptn = -1;
  if (ioctl(master, TIOCGPTN, &ptn) < 0) {
    printf("FAIL: ioctl TIOCGPTN failed\n");
    close(master);
    return 1;
  }
  printf("PTY allocated: /dev/pts/%d\n", ptn);

  int unlock = 0;
  if (ioctl(master, TIOCSPTLCK, &unlock) < 0) {
    printf("FAIL: ioctl TIOCSPTLCK failed\n");
    close(master);
    return 1;
  }

  char pts_path[32];
  snprintf(pts_path, sizeof(pts_path), "/dev/pts/%d", ptn);

  int slave = open(pts_path, O_RDWR);
  if (slave < 0) {
    printf("FAIL: open slave %s failed\n", pts_path);
    close(master);
    return 1;
  }

  const char *msg = "hello from pty master";
  write(master, msg, strlen(msg));

  char buf[64];
  memset(buf, 0, sizeof(buf));
  int n = read(slave, buf, sizeof(buf) - 1);
  printf("Slave received %d bytes: %s\n", n, buf);

  const char *reply = "reply from pty slave";
  write(slave, reply, strlen(reply));

  memset(buf, 0, sizeof(buf));
  n = read(master, buf, sizeof(buf) - 1);
  printf("Master received %d bytes: %s\n", n, buf);

  close(slave);
  close(master);

  printf("PTY test passed!\n");
  return 0;
}
