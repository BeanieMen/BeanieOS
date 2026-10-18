#include "syscall.h"

#define FBIOGET_VSCREENINFO 0x4600
#define FBIOGET_FSCREENINFO 0x4602

#define PROT_READ  0x1
#define PROT_WRITE 0x2
#define MAP_SHARED 0x01

#define O_RDWR 2

struct fb_bitfield {
  unsigned int offset;
  unsigned int length;
  unsigned int msb_right;
};

struct fb_var_screeninfo {
  unsigned int xres;
  unsigned int yres;
  unsigned int xres_virtual;
  unsigned int yres_virtual;
  unsigned int xoffset;
  unsigned int yoffset;
  unsigned int bits_per_pixel;
  unsigned int grayscale;
  struct fb_bitfield red;
  struct fb_bitfield green;
  struct fb_bitfield blue;
  struct fb_bitfield transp;
  unsigned int nonstd;
  unsigned int activate;
  unsigned int height;
  unsigned int width;
  unsigned int accel_flags;
  unsigned int pixclock;
  unsigned int left_margin;
  unsigned int right_margin;
  unsigned int upper_margin;
  unsigned int lower_margin;
  unsigned int hsync_len;
  unsigned int vsync_len;
  unsigned int sync;
  unsigned int vmode;
  unsigned int rotate;
  unsigned int colorspace;
  unsigned int reserved[4];
};

struct fb_fix_screeninfo {
  char id[16];
  unsigned long smem_start;
  unsigned int smem_len;
  unsigned int type;
  unsigned int type_aux;
  unsigned int visual;
  unsigned short xpanstep;
  unsigned short ypanstep;
  unsigned short ywrapstep;
  unsigned int line_length;
  unsigned long mmio_start;
  unsigned int mmio_len;
  unsigned int accel;
  unsigned short capabilities;
  unsigned short reserved[2];
};

static void print(const char *s) {
  unsigned long len = 0;
  while (s[len]) len++;
  sys_write(1, s, len);
}

int main(void) {
  print("Opening /dev/fb0...\n");
  long fd = sys_open("/dev/fb0", O_RDWR);
  if (fd < 0) {
    print("Failed to open /dev/fb0\n");
    return 1;
  }

  print("Opened /dev/fb0 successfully!\n");

  struct fb_var_screeninfo var;
  if (sys_ioctl((int)fd, FBIOGET_VSCREENINFO, &var) < 0) {
    print("Failed to get var screeninfo\n");
    return 1;
  }

  struct fb_fix_screeninfo fix;
  if (sys_ioctl((int)fd, FBIOGET_FSCREENINFO, &fix) < 0) {
    print("Failed to get fix screeninfo\n");
    return 1;
  }

  print("Got screeninfo. Mapping memory...\n");

  unsigned long screensize = fix.smem_len;
  unsigned int *fbp = (unsigned int *)sys_mmap(0, screensize, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  if ((long)fbp < 0) {
    print("Failed to mmap framebuffer\n");
    return 1;
  }

  print("Mapped framebuffer! Drawing RGB gradient...\n");

  unsigned int width = var.xres;
  unsigned int height = var.yres;
  unsigned int pitch_pixels = fix.line_length / 4;

  for (unsigned int y = 0; y < height; y++) {
    for (unsigned int x = 0; x < width; x++) {
      unsigned char r = (x * 255) / width;
      unsigned char g = (y * 255) / height;
      unsigned char b = 160;
      fbp[y * pitch_pixels + x] = (r << 16) | (g << 8) | b;
    }
  }

  print("Screen drawn! Sleeping for 5s (Press Ctrl+C to exit)...\n");
  struct timespec ts;
  ts.tv_sec = 5;
  ts.tv_nsec = 0;
  sys_nanosleep(&ts, 0);

  return 0;
}