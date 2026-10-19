#ifndef _BEANIE_FB_H
#define _BEANIE_FB_H

#define FBIOGET_VSCREENINFO 0x4600
#define FBIOGET_FSCREENINFO 0x4602

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

#endif