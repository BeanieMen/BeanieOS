#include "syscall.h"

int main(void) {
  sys_write(1, "loop: spinning (press Ctrl+C to test SIGINT termination)...\n", 60);
  struct timespec ts;
  ts.tv_sec = 1;
  ts.tv_nsec = 0;
  while (1) {
    sys_nanosleep(&ts, 0);
  }
  return 0;
}