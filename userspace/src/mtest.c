#include "syscall.h"

static void print_str(const char *s) {
  int len = 0;
  while (s[len]) len++;
  sys_write(1, s, len);
}

static void print_num(int n) {
  char buf[16];
  int idx = 0;
  if (n < 0) {
    print_str("-");
    n = -n;
  }
  if (n == 0) {
    print_str("0");
    return;
  }
  while (n > 0) {
    buf[idx++] = '0' + (n % 10);
    n /= 10;
  }
  for (int i = 0; i < idx / 2; i++) {
    char tmp = buf[i];
    buf[i] = buf[idx - 1 - i];
    buf[idx - 1 - i] = tmp;
  }
  buf[idx] = '\0';
  print_str(buf);
}

int main() {
  print_str("[mousetest] Opening /dev/input/mice...\n");

  int fd = sys_open("/dev/input/mice", 0);
  if (fd < 0) {
    fd = sys_open("/dev/mouse", 0);
  }
  if (fd < 0) {
    print_str("[mousetest] Failed to open mouse device!\n");
    return 1;
  }

  print_str("[mousetest] Mouse opened! Move or click the mouse (testing 20 events)...\n");

  for (int count = 0; count < 20; count++) {
    unsigned char packet[3];
    long n = sys_read(fd, packet, 3);
    if (n < 3) {
      continue;
    }

    int left = packet[0] & 1;
    int right = (packet[0] & 2) ? 1 : 0;
    int middle = (packet[0] & 4) ? 1 : 0;

    int dx = (packet[0] & 0x10) ? ((int)packet[1] - 256) : (int)packet[1];
    int dy = (packet[0] & 0x20) ? ((int)packet[2] - 256) : (int)packet[2];

    print_str("Event ");
    print_num(count + 1);
    print_str(": dx=");
    print_num(dx);
    print_str(" dy=");
    print_num(dy);
    print_str(" btn=[");
    if (left) print_str("L");
    if (middle) print_str("M");
    if (right) print_str("R");
    print_str("]\n");
  }

  print_str("[mousetest] Test complete!\n");
  sys_close(fd);
  return 0;
}
