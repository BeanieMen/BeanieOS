// BeanieOS userspace DHCPv4 client.
//
// Network configuration is policy, so it lives here in userspace, not in
// the kernel. dhcpc talks raw Ethernet frames through an AF_PACKET-style
// socket (the kernel mechanism), negotiates a lease, then applies the
// address and default route through the interface-configuration ioctls and
// writes /etc/resolv.conf for userspace resolvers.
//
// It then stays resident until the lease expires and clears the address
// and default route it installed, but only if the current configuration
// still matches exactly what it applied (a newer configuration is never
// clobbered). Renewal/rebind is deliberately not implemented: at expiry
// the interface goes unconfigured until dhcpc is run again, rather than
// keeping a stale address past its lease.

#include "syscall.h"

#define AF_PACKET 17
#define SOCK_RAW 3
#define SOCK_NONBLOCK 0x800

#define SIOCSIFADDR 0x8916
#define SIOCGIFADDR 0x8915
#define SIOCGIFHWADDR 0x8927
#define SIOCADDRT 0x890B
#define NET_IOC_GET_GW 0x89F1  // BeanieOS-specific default-route query

#define DHCP_DISCOVER 1
#define DHCP_OFFER 2
#define DHCP_REQUEST 3
#define DHCP_ACK 5
#define DHCP_NAK 6

static unsigned long slen(const char *s) {
  unsigned long n = 0;
  while (s[n]) n++;
  return n;
}

static void print(const char *s) { sys_write(1, s, slen(s)); }

static void print_dec(unsigned long n) {
  char buf[24];
  int i = 0;
  if (n == 0) { sys_write(1, "0", 1); return; }
  char d[24];
  int c = 0;
  while (n > 0) { d[c++] = '0' + (n % 10); n /= 10; }
  while (c > 0) buf[i++] = d[--c];
  sys_write(1, buf, i);
}

static void print_ip(const unsigned char *ip) {
  for (int i = 0; i < 4; i++) {
    if (i) print(".");
    print_dec(ip[i]);
  }
}

static void mset(void *dst, int c, unsigned long n) {
  unsigned char *d = dst;
  for (unsigned long i = 0; i < n; i++) d[i] = (unsigned char)c;
}

static void mcopy(void *dst, const void *src, unsigned long n) {
  unsigned char *d = dst;
  const unsigned char *s = src;
  for (unsigned long i = 0; i < n; i++) d[i] = s[i];
}

static unsigned char mac[6];

static unsigned short ip_checksum(const unsigned char *p, int len) {
  unsigned long sum = 0;
  while (len > 1) { sum += (p[0] << 8) | p[1]; p += 2; len -= 2; }
  if (len) sum += p[0] << 8;
  while (sum >> 16) sum = (sum & 0xffff) + (sum >> 16);
  return (unsigned short)~sum;
}

// Frame layout: eth 14 | ip 20 | udp 8 | bootp 236 | cookie 4 | opts
#define OFF_IP 14
#define OFF_UDP 34
#define OFF_BOOTP 42
#define OFF_OPTS (OFF_BOOTP + 240)
#define MIN_BOOTP_PAYLOAD 300

static int build_bootp(unsigned char *f, unsigned int xid, int msg_type,
                       const unsigned char *req_ip,
                       const unsigned char *server) {
  mset(f, 0, 768);
  // Ethernet
  mset(f, 0xff, 6);          // dst broadcast
  mcopy(f + 6, mac, 6);      // src
  f[12] = 0x08; f[13] = 0x00;
  // IPv4
  f[OFF_IP + 0] = 0x45;
  f[OFF_IP + 8] = 64;        // ttl
  f[OFF_IP + 9] = 17;        // udp
  // src 0.0.0.0 dst 255.255.255.255
  mset(f + OFF_IP + 16, 0xff, 4);
  // UDP
  f[OFF_UDP + 0] = 0; f[OFF_UDP + 1] = 68;
  f[OFF_UDP + 2] = 0; f[OFF_UDP + 3] = 67;
  // BOOTP
  unsigned char *b = f + OFF_BOOTP;
  b[0] = 1; b[1] = 1; b[2] = 6; b[3] = 0;
  b[4] = (xid >> 24) & 0xff; b[5] = (xid >> 16) & 0xff;
  b[6] = (xid >> 8) & 0xff;  b[7] = xid & 0xff;
  b[10] = 0x80;              // broadcast flag
  mcopy(b + 28, mac, 6);     // chaddr
  f[OFF_BOOTP + 236] = 99; f[OFF_BOOTP + 237] = 130;
  f[OFF_BOOTP + 238] = 83; f[OFF_BOOTP + 239] = 99;
  // options
  int o = OFF_OPTS;
  f[o++] = 53; f[o++] = 1; f[o++] = (unsigned char)msg_type;
  if (msg_type == DHCP_REQUEST) {
    f[o++] = 50; f[o++] = 4; mcopy(f + o, req_ip, 4); o += 4;
    f[o++] = 54; f[o++] = 4; mcopy(f + o, server, 4); o += 4;
  }
  f[o++] = 55; f[o++] = 4; f[o++] = 1; f[o++] = 3; f[o++] = 6; f[o++] = 51;
  f[o++] = 255;
  int payload = o - OFF_BOOTP;
  if (payload < MIN_BOOTP_PAYLOAD) payload = MIN_BOOTP_PAYLOAD;
  int udp_len = 8 + payload;
  int ip_len = 20 + udp_len;
  f[OFF_IP + 2] = (ip_len >> 8) & 0xff; f[OFF_IP + 3] = ip_len & 0xff;
  f[OFF_UDP + 4] = (udp_len >> 8) & 0xff; f[OFF_UDP + 5] = udp_len & 0xff;
  unsigned short cks = ip_checksum(f + OFF_IP, 20);
  f[OFF_IP + 10] = (cks >> 8) & 0xff; f[OFF_IP + 11] = cks & 0xff;
  int frame_len = OFF_IP + ip_len;
  if (frame_len < 60) frame_len = 60;
  return frame_len;
}

// Parse a received frame; on a DHCP reply matching xid, return the message
// type (2=offer, 5=ack, 6=nak) and fill yiaddr/server/router/mask/dns/lease.
static int parse_reply(const unsigned char *f, int n, unsigned int xid,
                       unsigned char *yiaddr, unsigned char *server,
                       unsigned char *router, unsigned char *mask,
                       unsigned char *dns, int *dns_count,
                       unsigned long *lease) {
  if (n < OFF_OPTS + 4) return 0;
  if (f[12] != 0x08 || f[13] != 0x00) return 0;
  if (f[OFF_IP + 9] != 17) return 0;
  int ihl = (f[OFF_IP] & 0x0f) * 4;
  int uoff = OFF_IP + ihl;
  if (f[uoff + 2] != 0 || f[uoff + 3] != 68) return 0; // to port 68
  const unsigned char *b = f + uoff + 8;
  if (n < uoff + 8 + 240) return 0;
  if (b[0] != 2) return 0; // reply
  unsigned int rx = ((unsigned)b[4] << 24) | ((unsigned)b[5] << 16) |
                    ((unsigned)b[6] << 8) | b[7];
  if (rx != xid) return 0;
  if (b[236] != 99 || b[237] != 130 || b[238] != 83 || b[239] != 99) return 0;
  mcopy(yiaddr, b + 16, 4);
  const unsigned char *opts = b + 240;
  int remaining = n - (uoff + 8 + 240);
  int msg = 0;
  *dns_count = 0;
  *lease = 0;
  for (int i = 0; i + 1 < remaining;) {
    unsigned code = opts[i];
    if (code == 255) break;
    if (code == 0) { i++; continue; }
    int l = opts[i + 1];
    if (i + 2 + l > remaining) break;
    const unsigned char *v = opts + i + 2;
    if (code == 53 && l >= 1) msg = v[0];
    else if (code == 54 && l >= 4) mcopy(server, v, 4);
    else if (code == 3 && l >= 4) mcopy(router, v, 4);
    else if (code == 1 && l >= 4) mcopy(mask, v, 4);
    else if (code == 6 && l >= 4) {
      int c = l / 4;
      if (c > 3) c = 3;
      mcopy(dns, v, c * 4);
      *dns_count = c;
    } else if (code == 51 && l >= 4) {
      *lease = ((unsigned long)v[0] << 24) | ((unsigned long)v[1] << 16) |
               ((unsigned long)v[2] << 8) | v[3];
    }
    i += 2 + l;
  }
  return msg;
}

// Read frames until a DHCP reply for xid arrives or the window closes.
// Returns the message type, or 0 on timeout.
static int await_reply(int fd, unsigned int xid, int timeout_ms,
                       unsigned char *yiaddr, unsigned char *server,
                       unsigned char *router, unsigned char *mask,
                       unsigned char *dns, int *dns_count,
                       unsigned long *lease) {
  unsigned char buf[1600];
  int elapsed = 0;
  while (elapsed < timeout_ms) {
    long n = sys_read(fd, buf, sizeof(buf));
    if (n > 0) {
      int msg = parse_reply(buf, (int)n, xid, yiaddr, server, router, mask,
                            dns, dns_count, lease);
      if (msg) return msg;
      continue;
    }
    sys_sleep_ms(5);
    elapsed += 5;
  }
  return 0;
}

static unsigned long parse_secs(const char *p) {
  unsigned long v = 0;
  while (*p >= '0' && *p <= '9') { v = v * 10 + (unsigned)(*p - '0'); p++; }
  return v;
}

// argv[1], when present, overrides the lease time in seconds. Test hook
// for exercising expiry without waiting for a real lease to run out.
int main(int argc, char **argv) {
  unsigned long lease_override = (argc > 1) ? parse_secs(argv[1]) : 0;
  int fd = sys_socket(AF_PACKET, SOCK_RAW | SOCK_NONBLOCK, 0);
  if (fd < 0) {
    print("NET_DHCP_FAIL reason=no-raw-socket\n");
    return 1;
  }
  if (sys_ioctl(fd, SIOCGIFHWADDR, mac) < 0) {
    print("NET_DHCP_FAIL reason=no-device\n");
    return 1;
  }

  unsigned int xid = 0xBEA00000u ^ ((unsigned)mac[3] << 16) ^
                     ((unsigned)mac[4] << 8) ^ mac[5];

  unsigned char frame[768];
  unsigned char yiaddr[4] = {0, 0, 0, 0};
  unsigned char server[4] = {0, 0, 0, 0};
  unsigned char router[4] = {0, 0, 0, 0};
  unsigned char mask[4] = {255, 255, 255, 0};
  unsigned char dns[12];
  int dns_count = 0;
  unsigned long lease = 0;

  // DISCOVER with backoff: 1s, 2s, 4s, 8s (~15s total).
  static const int discover_waits[] = {1000, 2000, 4000, 8000};
  int offer_seen = 0;
  for (int attempt = 0; attempt < 4 && !offer_seen; attempt++) {
    int flen = build_bootp(frame, xid, DHCP_DISCOVER, 0, 0);
    sys_write(fd, frame, flen);
    int msg = await_reply(fd, xid, discover_waits[attempt], yiaddr, server,
                          router, mask, dns, &dns_count, &lease);
    if (msg == DHCP_OFFER) offer_seen = 1;
    else if (msg == DHCP_NAK) break;
  }
  if (!offer_seen) {
    print("NET_DHCP_FAIL reason=no-server\n");
    sys_close(fd);
    return 1;
  }

  // REQUEST the offered address.
  static const int request_waits[] = {2000, 4000};
  int ack_seen = 0;
  for (int attempt = 0; attempt < 2 && !ack_seen; attempt++) {
    int flen = build_bootp(frame, xid, DHCP_REQUEST, yiaddr, server);
    sys_write(fd, frame, flen);
    int msg = await_reply(fd, xid, request_waits[attempt], yiaddr, server,
                          router, mask, dns, &dns_count, &lease);
    if (msg == DHCP_ACK) ack_seen = 1;
    else if (msg == DHCP_NAK) break;
  }
  if (!ack_seen) {
    print("NET_DHCP_FAIL reason=no-ack\n");
    sys_close(fd);
    return 1;
  }

  // Apply the lease through the kernel's configuration mechanism.
  unsigned char addr_mask[8];
  mcopy(addr_mask, yiaddr, 4);
  mcopy(addr_mask + 4, mask, 4);
  if (sys_ioctl(fd, SIOCSIFADDR, addr_mask) < 0) {
    print("NET_DHCP_FAIL reason=set-addr\n");
    sys_close(fd);
    return 1;
  }
  if (router[0] || router[1] || router[2] || router[3]) {
    if (sys_ioctl(fd, SIOCADDRT, router) < 0) {
      print("NET_DHCP_FAIL reason=set-route\n");
      sys_close(fd);
      return 1;
    }
  }

  // DNS servers are userspace policy: record them for resolvers.
  long mkrc = sys_mkdir("/etc");
  int rf = sys_open("/etc/resolv.conf", O_WRONLY | O_CREAT | O_TRUNC);
  if (rf < 0) {
    print("dhcpc: warning: could not write /etc/resolv.conf mkdir_rc=");
    print_dec(mkrc < 0 ? (unsigned long)-mkrc : (unsigned long)mkrc);
    print(" open_rc=");
    print_dec((unsigned long)-rf);
    print("\n");
  }
  if (rf >= 0) {
    for (int i = 0; i < dns_count; i++) {
      char line[40];
      int p = 0;
      const char *ns = "nameserver ";
      for (int j = 0; ns[j]; j++) line[p++] = ns[j];
      for (int j = 0; j < 4; j++) {
        unsigned v = dns[i * 4 + j];
        if (j) line[p++] = '.';
        char tmp[4];
        int t = 0;
        if (v == 0) tmp[t++] = '0';
        while (v) { tmp[t++] = '0' + (v % 10); v /= 10; }
        while (t > 0) line[p++] = tmp[--t];
      }
      line[p++] = '\n';
      sys_write(rf, line, p);
    }
    sys_close(rf);
  }

  int prefix = 0;
  for (int i = 0; i < 4; i++) {
    unsigned m = mask[i];
    while (m) { prefix += m & 1; m >>= 1; }
  }

  print("NET_DHCP_OK addr=");
  print_ip(yiaddr);
  print("/");
  print_dec(prefix);
  print(" gw=");
  print_ip(router);
  print(" dns=");
  if (dns_count) print_ip(dns); else print("none");
  print(" lease=");
  print_dec(lease);
  print("\n");

  // Stay resident until the lease expires (a lease of 0 is treated as
  // infinite), then clear only the configuration this run installed.
  unsigned long wait_s = lease_override ? lease_override : lease;
  if (lease == 0 && lease_override == 0) {
    // Infinite lease: nothing to expire.
    sys_close(fd);
    return 0;
  }
  unsigned long remaining_ms = wait_s * 1000UL;
  while (remaining_ms > 0) {
    unsigned long chunk = remaining_ms > 60000UL ? 60000UL : remaining_ms;
    sys_sleep_ms(chunk);
    remaining_ms -= chunk;
  }

  unsigned char cur_am[8] = {0};
  unsigned char cur_gw[4] = {0};
  if (sys_ioctl(fd, SIOCGIFADDR, cur_am) < 0 ||
      sys_ioctl(fd, NET_IOC_GET_GW, cur_gw) < 0) {
    // Cannot verify the current configuration; leave it untouched.
    print("NET_EXPIRY_SKIP reason=query-failed\n");
    sys_close(fd);
    return 0;
  }
  int addr_match = 1;
  for (int i = 0; i < 8; i++)
    if (cur_am[i] != addr_mask[i]) addr_match = 0;
  int gw_match = 1;
  for (int i = 0; i < 4; i++)
    if (cur_gw[i] != router[i]) gw_match = 0;
  if (addr_match && gw_match) {
    unsigned char zero8[8] = {0};
    unsigned char zero4[4] = {0};
    sys_ioctl(fd, SIOCSIFADDR, zero8);  // unconfigure addresses
    sys_ioctl(fd, SIOCADDRT, zero4);    // remove default route
    print("NET_EXPIRY_CLEARED addr=");
    print_ip(yiaddr);
    print("\n");
  } else {
    print("NET_EXPIRY_SKIP reason=reconfigured\n");
  }

  sys_close(fd);
  return 0;
}
