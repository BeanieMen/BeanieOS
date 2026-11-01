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


// ---- networking (userspace policy over kernel mechanisms) ----
// DHCP runs as the userspace client /bin/dhcpc; the DNS check below is a
// userspace resolver probe over the kernel's TCP mechanism (DNS over TCP,
// RFC 7766) so no resolver lives in the kernel.

struct sockaddr_in_net {
  unsigned short sin_family;
  unsigned short sin_port;
  unsigned int sin_addr;
  char sin_zero[8];
};

static unsigned short htons_(unsigned short v) { return (v << 8) | (v >> 8); }

static int parse_nameserver(unsigned char *ip) {
  long fd = sys_open("/etc/resolv.conf", O_RDONLY);
  if (fd < 0) return -2;
  char buf[256];
  long n = sys_read(fd, buf, sizeof(buf) - 1);
  sys_close(fd);
  if (n <= 0) return -3;
  buf[n] = 0;
  const char *key = "nameserver ";
  long i = 0;
  for (; i + 11 <= n; i++) {
    long j = 0;
    while (j < 11 && buf[i + j] == key[j]) j++;
    if (j == 11) break;
  }
  if (i + 11 > n) return -1;
  i += 11;
  for (int oct = 0; oct < 4; oct++) {
    unsigned v = 0;
    int digits = 0;
    while (buf[i] >= '0' && buf[i] <= '9') {
      v = v * 10 + (buf[i] - '0');
      i++;
      digits++;
    }
    if (!digits || v > 255) return -1;
    ip[oct] = (unsigned char)v;
    if (oct < 3 && buf[i] != '.') return -1;
    i++;
  }
  return 0;
}

static void dns_check(void) {
  unsigned char ns[4];
  int pns = parse_nameserver(ns);
  if (pns == -2) {
    print("NET_DNS_FAIL reason=resolvconf-open\n");
    return;
  }
  if (pns == -3) {
    print("NET_DNS_FAIL reason=resolvconf-empty\n");
    return;
  }
  if (pns < 0) {
    print("NET_DNS_FAIL reason=resolvconf-parse\n");
    return;
  }
  int s = sys_socket(2, 1, 0); // AF_INET, SOCK_STREAM
  if (s < 0) {
    print("NET_DNS_FAIL reason=socket\n");
    return;
  }
  struct sockaddr_in_net addr;
  addr.sin_family = 2;
  addr.sin_port = htons_(53);
  unsigned char *a = (unsigned char *)&addr.sin_addr;
  a[0] = ns[0]; a[1] = ns[1]; a[2] = ns[2]; a[3] = ns[3];
  for (int i = 0; i < 8; i++) addr.sin_zero[i] = 0;
  if (sys_connect(s, (struct sockaddr_un *)&addr, sizeof(addr)) < 0) {
    print("NET_DNS_FAIL reason=connect\n");
    sys_close(s);
    return;
  }
  // DNS query for example.com A, with the RFC 7766 TCP length prefix.
  unsigned char q[31];
  q[0] = 0; q[1] = 29;                     // TCP length prefix: 29-byte msg
  q[2] = 0x12; q[3] = 0x34;                // id
  q[4] = 0x01; q[5] = 0x00;                // flags: RD
  q[6] = 0; q[7] = 1;                      // qdcount
  q[8] = 0; q[9] = 0; q[10] = 0; q[11] = 0; q[12] = 0; q[13] = 0;
  unsigned char qname[13] = {7, 'e', 'x', 'a', 'm', 'p', 'l', 'e', 3, 'c', 'o', 'm', 0};
  for (int i = 0; i < 13; i++) q[14 + i] = qname[i];
  q[27] = 0; q[28] = 1;                    // A
  q[29] = 0; q[30] = 1;                    // IN
  sys_write(s, q, 31);

  unsigned char resp[512];
  long got = 0;
  while (got < 2) {
    long r = sys_read(s, resp + got, sizeof(resp) - got);
    if (r <= 0) break;
    got += r;
  }
  if (got >= 2) {
    long want = 2 + (((long)resp[0] << 8) | resp[1]);
    if (want > (long)sizeof(resp)) want = sizeof(resp);
    while (got < want) {
      long r = sys_read(s, resp + got, want - got);
      if (r <= 0) break;
      got += r;
    }
  }
  sys_close(s);
  if (got < 8) {
    print("NET_DNS_FAIL reason=query\n");
    return;
  }
  int answers = (resp[2 + 6] << 8) | resp[2 + 7];
  if (answers <= 0) {
    print("NET_DNS_FAIL reason=empty\n");
    return;
  }
  print("NET_DNS_OK answers=");
  char nb[8];
  int c = 0;
  while (answers > 0) { nb[c++] = '0' + (answers % 10); answers /= 10; }
  while (c > 0) { char ch[2] = {nb[--c], 0}; print(ch); }
  print("\n");
}

// Negative coverage for the raw-socket and interface-ioctl mechanism:
// invalid inputs are rejected, dup'd aliases share one socket until the
// last close, and cleanup leaves a fresh socket usable.
static void raw_suite(void) {
  int bad = sys_socket(17, 1, 0);  // SOCK_STREAM on AF_PACKET: not supported
  check(bad < 0, "raw: non-raw type rejected", bad);
  int fd = sys_socket(17, 3, 0);
  check(fd >= 0, "raw: socket opens", fd);
  if (fd >= 0) {
    char tiny[5] = {0, 0, 0, 0, 0};
    long w = sys_write(fd, tiny, 5);
    check(w < 0, "raw: short frame rejected", w);
    char big[1600];
    for (int i = 0; i < 1600; i++) big[i] = 0;
    w = sys_write(fd, big, sizeof(big));
    check(w < 0, "raw: oversize frame rejected", w);
    int d2 = sys_dup(fd);
    check(d2 >= 0, "raw: dup alias", d2);
    sys_close(fd);
    if (d2 >= 0) {
      // The socket must survive closing one alias: a valid frame still
      // goes out through the remaining one.
      unsigned char frame[60];
      for (int i = 0; i < 6; i++) frame[i] = 0xff;         // dst broadcast
      for (int i = 0; i < 6; i++) frame[6 + i] = 0x52;     // src (bogus)
      frame[12] = 0x88; frame[13] = 0xb5;                  // local ethertype
      for (int i = 14; i < 60; i++) frame[i] = 0;
      w = sys_write(d2, frame, sizeof(frame));
      check(w == 60, "raw: alias usable after close", w);
      sys_close(d2);
    }
    // After the last close, a fresh raw socket must still work.
    int fd2 = sys_socket(17, 3, 0);
    check(fd2 >= 0, "raw: reopen after cleanup", fd2);
    if (fd2 >= 0) sys_close(fd2);
  }
  int cfd = sys_socket(17, 3, 0);
  check(cfd >= 0, "raw: ioctl socket", cfd);
  if (cfd >= 0) {
    unsigned char am[8] = {10, 0, 2, 15, 255, 255, 0, 255};
    long r = sys_ioctl(cfd, 0x8916, am);  // non-contiguous mask
    check(r < 0, "ioctl: non-contiguous mask rejected", r);
    unsigned char am0[8] = {10, 0, 2, 15, 0, 0, 0, 0};
    r = sys_ioctl(cfd, 0x8916, am0);      // zero mask, nonzero addr
    check(r < 0, "ioctl: zero mask rejected", r);
    r = sys_ioctl(cfd, 0x8927, (void *)1);  // bad user pointer
    check(r < 0, "ioctl: bad pointer rejected", r);
    unsigned char mac[6];
    r = sys_ioctl(cfd, 0x8927, mac);        // sane call still works
    check(r == 0, "ioctl: hwaddr query works", r);
    sys_close(cfd);
  }
}

// Run dhcpc with a tiny test lease; at expiry it must clear exactly the
// configuration it installed, so a TCP connect afterwards must fail.
static void expiry_test(void) {
  long pid = sys_fork();
  if (pid == 0) {
    char *a[] = {"/bin/dhcpc", "2", 0};
    char *e[] = {0};
    sys_execve("/bin/dhcpc", a, e);
    print("NET_EXPIRY_FAIL reason=exec\n");
    sys_exit(127);
  }
  int st = 0;
  sys_wait4(pid, &st, 0, 0);
  check(((st >> 8) & 0xff) == 0, "net: expiry run exits cleanly",
        (st >> 8) & 0xff);
  int s = sys_socket(2, 1, 0);  // AF_INET, SOCK_STREAM
  if (s < 0) {
    check(0, "net: post-expiry socket", s);
    return;
  }
  struct sockaddr_in_net addr;
  addr.sin_family = 2;
  addr.sin_port = htons_(53);
  unsigned char *a = (unsigned char *)&addr.sin_addr;
  a[0] = 10; a[1] = 0; a[2] = 2; a[3] = 3;
  for (int i = 0; i < 8; i++) addr.sin_zero[i] = 0;
  long r = sys_connect(s, (struct sockaddr_un *)&addr, sizeof(addr));
  check(r < 0, "net: config cleared at lease expiry", r);
  sys_close(s);
}

static void net_suite(void) {
  print("-- net\n");
  raw_suite();
  long pid = sys_fork();
  if (pid == 0) {
    char *dargv[] = {"/bin/dhcpc", 0};
    char *denvp[] = {0};
    sys_execve("/bin/dhcpc", dargv, denvp);
    print("NET_DHCP_FAIL reason=exec\n");
    sys_exit(127);
  }
  // dhcpc stays resident after configuring; wait until the interface has
  // an address instead of for its exit. (Query the kernel, not
  // /etc/resolv.conf: the file's contents can survive a reboot.)
  int ready = 0;
  int pfd = sys_socket(17, 3, 0);
  for (int i = 0; i < 600 && !ready; i++) {  // ~30s
    unsigned char am[8] = {0};
    if (pfd >= 0 && sys_ioctl(pfd, 0x8915, am) == 0 &&
        (am[0] || am[1] || am[2] || am[3]))
      ready = 1;
    else
      sys_sleep_ms(50);
  }
  if (pfd >= 0) sys_close(pfd);
  if (!ready) {
    print("NET_DHCP_FAIL reason=timeout\n");
    print("NET_DNS_SKIP reason=no-lease\n");
    return;
  }
  dns_check();
  expiry_test();
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
    net_suite();
    print("TESTS_DONE\n");
    for (;;) sys_sleep_ms(1000);
  }

  print("TESTINIT START\n");
  rename_suite();
  fsync_suite();
  prot_suite();
  net_suite();

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
