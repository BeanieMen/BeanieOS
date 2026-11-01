// BeanieOS regression tests: rename, fsync/sync durability semantics and
// mmap protection enforcement. Runs as /bin/init on the test image (no
// input needed) and is also runnable from the shell via `run testinit`.
//
// Phase 1 (no /syncdata/PERSIST.TXT): runs all suites, writes and flushes
// PERSIST.TXT, prints SYNC_READY then TESTS_DONE.
// Phase 2 (PERSIST.TXT exists, e.g. after a hard reset): verifies the
// flushed file survived, prints PERSIST_VERIFY PASS/FAIL and TESTS_DONE.

#include "syscall.h"

#define SYS_FSYNC 74
#define SYS_FDATASYNC 75
#define SYS_SYNC 162
#define SYS_RENAME 82

#define PROT_READ 0x1
#define PROT_WRITE 0x2
#define PROT_EXEC 0x4
#define MAP_PRIVATE 0x02
#define MAP_ANON 0x20

static unsigned long slen(const char *s) {
  unsigned long n = 0;
  while (s[n]) n++;
  return n;
}

static void print(const char *s) { sys_write(1, s, slen(s)); }

static int seq(const char *a, const char *b) {
  while (*a && *a == *b) { a++; b++; }
  return *(const unsigned char *)a - *(const unsigned char *)b;
}

static int failures = 0;

static void fail(const char *name, long got) {
  failures++;
  print("FAIL ");
  print(name);
  print(" got=");
  char buf[24];
  int i = 0;
  long n = got;
  if (n < 0) { buf[i++] = '-'; n = -n; }
  char d[24];
  int c = 0;
  if (n == 0) d[c++] = '0';
  while (n > 0) { d[c++] = '0' + (n % 10); n /= 10; }
  while (c > 0) buf[i++] = d[--c];
  buf[i++] = '\n';
  sys_write(1, buf, i);
}

static void ok(const char *name) { print("ok "); print(name); print("\n"); }

static void check(int cond, const char *name, long got) {
  if (cond) ok(name); else fail(name, got);
}

static long sys_rename(const char *oldp, const char *newp) {
  return syscall(SYS_RENAME, (long)oldp, (long)newp, 0);
}
static long sys_fsync(long fd) { return syscall(SYS_FSYNC, fd, 0, 0); }
static long sys_fdatasync(long fd) { return syscall(SYS_FDATASYNC, fd, 0, 0); }
static long sys_sync(void) { return syscall(SYS_SYNC, 0, 0, 0); }

static long write_file(const char *path, const char *data) {
  long fd = sys_open(path, O_WRONLY | O_CREAT | O_TRUNC);
  if (fd < 0) return fd;
  long w = sys_write(fd, data, slen(data));
  sys_close(fd);
  return w;
}

static long read_file(const char *path, char *buf, long max) {
  long fd = sys_open(path, O_RDONLY);
  if (fd < 0) return fd;
  long n = sys_read(fd, buf, max);
  sys_close(fd);
  return n;
}

static const char *PAYLOAD = "PAYLOAD-0123456789";

static void rename_suite(void) {
  print("-- rename suite --\n");
  sys_mkdir("/tname");
  sys_unlink("/tname/ONE.TXT");
  sys_unlink("/tname/TWO.TXT");

  check(write_file("/tname/ONE.TXT", "aaa") == 3, "rename: setup write", 0);
  check(sys_rename("/tname/ONE.TXT", "/tname/TWO.TXT") == 0, "rename: basic move rc", 0);
  check(sys_open("/tname/ONE.TXT", O_RDONLY) == -2, "rename: source gone", 0);
  char buf[64];
  long n = read_file("/tname/TWO.TXT", buf, 63);
  buf[n > 0 ? n : 0] = 0;
  check(n == 3 && seq(buf, "aaa") == 0, "rename: content moved", n);

  check(write_file("/tname/THREE.TXT", "BBB") == 3, "rename: replace setup", 0);
  check(sys_rename("/tname/THREE.TXT", "/tname/TWO.TXT") == 0, "rename: replace rc", 0);
  n = read_file("/tname/TWO.TXT", buf, 63);
  buf[n > 0 ? n : 0] = 0;
  check(n == 3 && seq(buf, "BBB") == 0, "rename: replace content", n);
  check(sys_open("/tname/THREE.TXT", O_RDONLY) == -2, "rename: replaced source gone", 0);

  long rc = sys_rename("/tname/NOPE.TXT", "/tname/X.TXT");
  check(rc == -2, "rename: missing source -> ENOENT", rc);

  sys_mkdir("/tname/D1");
  rc = sys_rename("/tname/TWO.TXT", "/tname/D1");
  check(rc == -21, "rename: file over dir -> EISDIR", rc);
  rc = sys_rename("/tname/D1", "/tname/TWO.TXT");
  check(rc == -20, "rename: dir over file -> ENOTDIR", rc);

  sys_mkdir("/tname/D2");
  check(write_file("/tname/D2/F.TXT", "x") == 1, "rename: dir setup", 0);
  rc = sys_rename("/tname/D1", "/tname/D2");
  check(rc == -39, "rename: dir over nonempty dir -> ENOTEMPTY", rc);
  n = read_file("/tname/D2/F.TXT", buf, 63);
  check(n == 1, "rename: rejected target intact", n);
  sys_unlink("/tname/D2/F.TXT");
  rc = sys_rename("/tname/D1", "/tname/D2");
  check(rc == 0, "rename: dir over empty dir", rc);
  check(sys_open("/tname/D1", O_RDONLY) == -2, "rename: old dir gone", 0);

  sys_mkdir("/tname/SUB");
  check(write_file("/tname/FOUR.TXT", "444") == 3, "rename: xdir setup", 0);
  rc = sys_rename("/tname/FOUR.TXT", "/tname/SUB/FOUR.TXT");
  check(rc == 0, "rename: cross-directory move rc", rc);
  check(sys_open("/tname/FOUR.TXT", O_RDONLY) == -2, "rename: xdir source gone", 0);
  n = read_file("/tname/SUB/FOUR.TXT", buf, 63);
  buf[n > 0 ? n : 0] = 0;
  check(n == 3 && seq(buf, "444") == 0, "rename: xdir content", n);

  rc = sys_rename("/tname/SUB", "/tname/SUB2");
  check(rc == 0, "rename: directory move rc", rc);
  n = read_file("/tname/SUB2/FOUR.TXT", buf, 63);
  check(n == 3, "rename: contents follow moved dir", n);
  check(write_file("/tname/SUB2/VIA.TXT", "v") == 1, "rename: write inside moved dir", 0);
  n = read_file("/tname/SUB2/VIA.TXT", buf, 63);
  check(n == 1, "rename: readback inside moved dir", n);

  rc = sys_rename("/tname/SUB2", "/tname/SUB2/INNER");
  check(rc == -22, "rename: dir into itself -> EINVAL", rc);

  rc = sys_rename("/tname/TWO.TXT", "/tname/TWO.TXT");
  check(rc == 0, "rename: self rename rc", rc);
  n = read_file("/tname/TWO.TXT", buf, 63);
  check(n == 3, "rename: self rename keeps file", n);

  // Open-handle behavior: a writer that stays open across a rename must
  // keep updating the (new) directory entry's size.
  long fd = sys_open("/tname/FIVE.TXT", O_RDWR | O_CREAT | O_TRUNC);
  check(fd >= 0, "rename: open-handle setup", fd);
  check(sys_write(fd, "ZZ", 2) == 2, "rename: open-handle write", 0);
  rc = sys_rename("/tname/FIVE.TXT", "/tname/SIX.TXT");
  check(rc == 0, "rename: rename with open handle rc", rc);
  check(sys_write(fd, "Z", 1) == 1, "rename: write via old handle", 0);
  sys_close(fd);
  n = read_file("/tname/SIX.TXT", buf, 63);
  buf[n > 0 ? n : 0] = 0;
  check(n == 3 && seq(buf, "ZZZ") == 0, "rename: size honest after open rename", n);

  // Cross-directory open-writer: the size write-back must follow the entry
  // to its new directory, and must not corrupt the entry that reuses the
  // old slot.
  sys_mkdir("/tname/ODIR");
  fd = sys_open("/tname/ODIR/SEVEN.TXT", O_RDWR | O_CREAT | O_TRUNC);
  check(fd >= 0, "rename: xdir open-handle setup", fd);
  check(sys_write(fd, "YY", 2) == 2, "rename: xdir open-handle write", 0);
  rc = sys_rename("/tname/ODIR/SEVEN.TXT", "/tname/EIGHT.TXT");
  check(rc == 0, "rename: xdir rename with open handle rc", rc);
  long fd2 = sys_open("/tname/ODIR/DECOY.TXT", O_RDWR | O_CREAT | O_TRUNC);
  check(fd2 >= 0, "rename: xdir decoy setup", fd2);
  check(sys_write(fd2, "dddd", 4) == 4, "rename: xdir decoy write", 0);
  sys_close(fd2);
  check(sys_write(fd, "Y", 1) == 1, "rename: xdir write via old handle", 0);
  sys_close(fd);
  n = read_file("/tname/EIGHT.TXT", buf, 63);
  buf[n > 0 ? n : 0] = 0;
  check(n == 3 && seq(buf, "YYY") == 0, "rename: xdir size honest after open rename", n);
  n = read_file("/tname/ODIR/DECOY.TXT", buf, 63);
  buf[n > 0 ? n : 0] = 0;
  check(n == 4 && seq(buf, "dddd") == 0, "rename: xdir decoy entry uncorrupted", n);
}

static void fsync_suite(void) {
  print("-- fsync suite --\n");
  long rc = sys_fsync(-1);
  check(rc == -9, "fsync: bad fd -1 -> EBADF", rc);
  rc = sys_fsync(9999);
  check(rc == -9, "fsync: bad fd 9999 -> EBADF", rc);
  rc = sys_fdatasync(9999);
  check(rc == -9, "fdatasync: bad fd -> EBADF", rc);

  sys_mkdir("/syncdata");
  long fd = sys_open("/syncdata/TMP.TXT", O_RDWR | O_CREAT | O_TRUNC);
  check(fd >= 0, "fsync: setup open", fd);
  check(sys_write(fd, "data", 4) == 4, "fsync: setup write", 0);
  check(sys_fsync(fd) == 0, "fsync: valid fd rc", 0);
  check(sys_fdatasync(fd) == 0, "fdatasync: valid fd rc", 0);
  check(sys_sync() == 0, "sync: rc", 0);
  sys_close(fd);

  int sv[2] = {-1, -1};
  if (sys_socketpair(AF_UNIX, SOCK_STREAM, 0, sv) == 0) {
    rc = sys_fsync(sv[0]);
    check(rc == -22, "fsync: socket -> EINVAL", rc);
    sys_close(sv[0]);
    sys_close(sv[1]);
  } else {
    print("skip fsync socket case (socketpair failed)\n");
  }
}

static char *g_ro;
static char *g_nx;
static char *g_none;
static char *g_rwx;
static char *g_cow;

// Returns child exit status word from wait4; BeanieOS encodes a
// fault-killed child as signaled: low 7 bits hold the signal (11 = SIGSEGV).
static long run_child(void (*fn)(void)) {
  long pid = sys_fork();
  if (pid == 0) {
    fn();
    sys_exit(0);
  }
  int st = 0;
  sys_wait4(pid, &st, 0, 0);
  return st;
}

static void child_write_ro(void) { *(volatile char *)g_ro = 1; }
static void child_call_nx(void) { ((void (*)(void))g_nx)(); }
static void child_read_none(void) {
  volatile char c = *(volatile char *)g_none;
  (void)c;
}
static void child_call_rwx(void) { ((void (*)(void))g_rwx)(); }
static void child_cow_write(void) {
  g_cow[0] = 'B';
  if (g_cow[0] == 'B') sys_exit(0);
  sys_exit(3);
}

static void prot_suite(void) {
  print("-- mmap protection suite --\n");

  char *rw = sys_mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
  check((long)rw > 0, "mmap: RW maps", (long)rw);
  rw[0] = 42;
  check(rw[0] == 42, "mmap: RW read/write", 0);

  g_ro = sys_mmap(0, 4096, PROT_READ, MAP_PRIVATE | MAP_ANON, -1, 0);
  check((long)g_ro > 0, "mmap: read-only maps", (long)g_ro);
  check(g_ro[0] == 0, "mmap: read-only readable", g_ro[0]);
  long st = run_child(child_write_ro);
  check((st & 0x7f) == 11, "mmap: write to read-only kills (SIGSEGV)", st);

  g_nx = sys_mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
  check((long)g_nx > 0, "mmap: NX maps", (long)g_nx);
  g_nx[0] = 0xC3; // ret
  st = run_child(child_call_nx);
  check((st & 0x7f) == 11, "mmap: exec of NX page kills (SIGSEGV)", st);

  g_rwx = sys_mmap(0, 4096, PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANON, -1, 0);
  check((long)g_rwx > 0, "mmap: RWX maps", (long)g_rwx);
  g_rwx[0] = 0xC3; // ret
  st = run_child(child_call_rwx);
  check(st == 0, "mmap: exec of RWX page works", st);

  g_none = sys_mmap(0, 4096, 0, MAP_PRIVATE | MAP_ANON, -1, 0);
  check((long)g_none > 0, "mmap: PROT_NONE reserves", (long)g_none);
  st = run_child(child_read_none);
  check((st & 0x7f) == 11, "mmap: read of PROT_NONE kills (SIGSEGV)", st);
  check(sys_munmap(g_none, 4096) == 0, "mmap: munmap PROT_NONE", 0);

  g_cow = sys_mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
  g_cow[0] = 'A';
  st = run_child(child_cow_write);
  check(st == 0, "mmap: COW child write/read", st);
  check(g_cow[0] == 'A', "mmap: COW parent unaffected", g_cow[0]);
}

int main(int argc, char **argv) {
  (void)argc;
  (void)argv;

  char buf[64];
  long n = read_file("/syncdata/PERSIST.TXT", buf, 63);
  if (n > 0) {
    buf[n] = 0;
    if (seq(buf, PAYLOAD) == 0) {
      print("PERSIST_VERIFY PASS\n");
    } else {
      print("PERSIST_VERIFY FAIL\n");
      failures++;
    }
    print("TESTS_DONE\n");
    for (;;) sys_sleep_ms(1000);
  }

  print("TESTINIT START\n");
  rename_suite();
  fsync_suite();
  prot_suite();

  // Durability phase: write + flush + sync, then report readiness so the
  // harness can hard-kill the VM and verify on the next boot.
  long fd = sys_open("/syncdata/PERSIST.TXT", O_RDWR | O_CREAT | O_TRUNC);
  if (fd >= 0) {
    sys_write(fd, PAYLOAD, slen(PAYLOAD));
    sys_fsync(fd);
    sys_close(fd);
    sys_sync();
    print("SYNC_READY\n");
  } else {
    fail("persist setup", fd);
  }

  if (failures == 0) {
    print("TESTS_DONE rc=0\n");
  } else {
    print("TESTS_DONE rc=1\n");
  }
  for (;;) sys_sleep_ms(1000);
}
