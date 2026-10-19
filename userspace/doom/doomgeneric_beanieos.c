#include "doomgeneric.h"
#include "doomkeys.h"
#include "fb.h"

#include <stdint.h>
#include <fcntl.h>
#include <poll.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static uint32_t *fb_ptr = NULL;
static uint32_t fb_width = 0;
static uint32_t fb_height = 0;
static uint32_t fb_pitch = 0;
static int fb_scale = 1;
static int fb_out_w = 0;
static int fb_out_h = 0;
static int fb_start_x = 0;
static int fb_start_y = 0;
static int kbd_fd = -1;
static int mouse_fd = -1;

static inline uint64_t rdtsc(void) {
  uint32_t lo, hi;
  __asm__ __volatile__ ("rdtsc" : "=a"(lo), "=d"(hi));
  return ((uint64_t)hi << 32) | lo;
}

static uint64_t tsc_freq = 0;
static uint64_t tsc_base = 0;

static void init_timer(void) {
  struct timespec ts1, ts2;
  clock_gettime(CLOCK_MONOTONIC, &ts1);
  uint64_t start_tsc = rdtsc();
  struct timespec sleep_ts = { 0, 10000000 }; // 10ms
  nanosleep(&sleep_ts, NULL);
  clock_gettime(CLOCK_MONOTONIC, &ts2);
  uint64_t end_tsc = rdtsc();

  int64_t elapsed_ns = (ts2.tv_sec - ts1.tv_sec) * 1000000000LL + (ts2.tv_nsec - ts1.tv_nsec);
  if (elapsed_ns > 0 && end_tsc > start_tsc) {
    tsc_freq = ((end_tsc - start_tsc) * 1000000000LL) / elapsed_ns;
  }
  tsc_base = rdtsc();
}

void DG_Init() {
  kbd_fd = open("/dev/kbd", O_RDONLY);
  mouse_fd = open("/dev/mouse", O_RDONLY);

  int fd = open("/dev/fb0", O_RDWR);
  if (fd < 0) return;

  struct fb_var_screeninfo var;
  struct fb_fix_screeninfo fix;

  if (ioctl(fd, FBIOGET_VSCREENINFO, &var) < 0) return;
  if (ioctl(fd, FBIOGET_FSCREENINFO, &fix) < 0) return;

  fb_width = var.xres;
  fb_height = var.yres;
  fb_pitch = fix.line_length / 4;

  int scale_x = fb_width / DOOMGENERIC_RESX;
  int scale_y = fb_height / DOOMGENERIC_RESY;
  fb_scale = scale_x < scale_y ? scale_x : scale_y;
  if (fb_scale < 1) fb_scale = 1;

  fb_out_w = DOOMGENERIC_RESX * fb_scale;
  fb_out_h = DOOMGENERIC_RESY * fb_scale;
  fb_start_x = (fb_width - fb_out_w) / 2;
  fb_start_y = (fb_height - fb_out_h) / 2;

  fb_ptr = (uint32_t *)mmap(0, fix.smem_len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  close(fd);

  init_timer();
}

void DG_DrawFrame() {
  if (!fb_ptr) return;

  if (fb_scale == 2) {
    for (int y = 0; y < DOOMGENERIC_RESY; y++) {
      const uint32_t *src_row = &DG_ScreenBuffer[y * DOOMGENERIC_RESX];
      uint64_t *dst0 = (uint64_t *)&fb_ptr[(fb_start_y + y * 2) * fb_pitch + fb_start_x];
      uint64_t *dst1 = (uint64_t *)&fb_ptr[(fb_start_y + y * 2 + 1) * fb_pitch + fb_start_x];
      for (int x = 0; x < DOOMGENERIC_RESX; x++) {
        uint32_t c = src_row[x];
        uint64_t pair = (uint64_t)c | ((uint64_t)c << 32);
        dst0[x] = pair;
        dst1[x] = pair;
      }
    }
    return;
  }

  if (fb_scale == 1) {
    for (int y = 0; y < DOOMGENERIC_RESY; y++) {
      const uint32_t *src_row = &DG_ScreenBuffer[y * DOOMGENERIC_RESX];
      uint32_t *dst = &fb_ptr[(fb_start_y + y) * fb_pitch + fb_start_x];
      memcpy(dst, src_row, DOOMGENERIC_RESX * sizeof(uint32_t));
    }
    return;
  }

  static uint32_t line_buf[2048];
  for (int y = 0; y < DOOMGENERIC_RESY; y++) {
    const uint32_t *src_row = &DG_ScreenBuffer[y * DOOMGENERIC_RESX];
    uint32_t *lb = line_buf;
    for (int x = 0; x < DOOMGENERIC_RESX; x++) {
      uint32_t color = src_row[x];
      for (int sx = 0; sx < fb_scale; sx++) {
        *lb++ = color;
      }
    }
    for (int sy = 0; sy < fb_scale; sy++) {
      uint32_t *dst = &fb_ptr[(fb_start_y + y * fb_scale + sy) * fb_pitch + fb_start_x];
      memcpy(dst, line_buf, fb_out_w * sizeof(uint32_t));
    }
  }
}

void DG_SleepMs(uint32_t ms) {
  struct timespec ts;
  ts.tv_sec = ms / 1000;
  ts.tv_nsec = (ms % 1000) * 1000000;
  nanosleep(&ts, NULL);
}

uint32_t DG_GetTicksMs() {
  if (tsc_freq > 0) {
    return (uint32_t)((rdtsc() - tsc_base) * 1000 / tsc_freq);
  }
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (uint32_t)(ts.tv_sec * 1000 + ts.tv_nsec / 1000000);
}

typedef struct {
  int pressed;
  unsigned char key;
} KeyEvent;

#define EVENT_QUEUE_SIZE 64
static KeyEvent s_events[EVENT_QUEUE_SIZE];
static int s_eq_head = 0;
static int s_eq_tail = 0;

static uint8_t s_key_down[256];
static uint32_t s_key_press_time[256];

#define KEY_RELEASE_TIMEOUT_MS 160

static void queue_key_event(int pressed, unsigned char key) {
  int next = (s_eq_head + 1) % EVENT_QUEUE_SIZE;
  if (next != s_eq_tail) {
    s_events[s_eq_head].pressed = pressed;
    s_events[s_eq_head].key = key;
    s_eq_head = next;
  }
}

static unsigned char convert_char_to_doom(unsigned char c) {
  if (c == 'w' || c == 'W') return KEY_UPARROW;
  if (c == 's' || c == 'S') return KEY_DOWNARROW;
  if (c == 'a' || c == 'A') return KEY_LEFTARROW;
  if (c == 'd' || c == 'D') return KEY_RIGHTARROW;
  if (c == 'q' || c == 'Q' || c == ',') return KEY_STRAFE_L;
  if (c == 'e' || c == 'E' || c == '.') return KEY_STRAFE_R;
  if (c == ' ') return KEY_USE;
  if (c == '\n' || c == '\r') return KEY_ENTER;
  if (c == 27) return KEY_ESCAPE;
  if (c == 'f' || c == 'F') return KEY_FIRE;
  if (c == '\t') return KEY_TAB;
  if (c == 0x08 || c == 0x7f) return KEY_BACKSPACE;
  if (c >= '1' && c <= '7') return c;
  return c;
}

static void poll_keys() {
  uint32_t now = DG_GetTicksMs();
  struct pollfd pfd = { .fd = 0, .events = POLLIN };

  while (poll(&pfd, 1, 0) > 0) {
    unsigned char c;
    if (read(0, &c, 1) <= 0) break;

    unsigned char k = 0;
    if (c == 27) {
      if (poll(&pfd, 1, 0) > 0) {
        unsigned char c2;
        if (read(0, &c2, 1) > 0 && c2 == '[') {
          if (poll(&pfd, 1, 0) > 0) {
            unsigned char c3;
            if (read(0, &c3, 1) > 0) {
              if (c3 == 'A') k = KEY_UPARROW;
              else if (c3 == 'B') k = KEY_DOWNARROW;
              else if (c3 == 'C') k = KEY_RIGHTARROW;
              else if (c3 == 'D') k = KEY_LEFTARROW;
            }
          }
        }
      }
      if (!k) k = KEY_ESCAPE;
    } else {
      k = convert_char_to_doom(c);
    }

    if (k) {
      if (k == KEY_UPARROW && s_key_down[KEY_DOWNARROW]) {
        s_key_down[KEY_DOWNARROW] = 0;
        queue_key_event(0, KEY_DOWNARROW);
      } else if (k == KEY_DOWNARROW && s_key_down[KEY_UPARROW]) {
        s_key_down[KEY_UPARROW] = 0;
        queue_key_event(0, KEY_UPARROW);
      } else if (k == KEY_LEFTARROW && s_key_down[KEY_RIGHTARROW]) {
        s_key_down[KEY_RIGHTARROW] = 0;
        queue_key_event(0, KEY_RIGHTARROW);
      } else if (k == KEY_RIGHTARROW && s_key_down[KEY_LEFTARROW]) {
        s_key_down[KEY_LEFTARROW] = 0;
        queue_key_event(0, KEY_LEFTARROW);
      } else if (k == KEY_STRAFE_L && s_key_down[KEY_STRAFE_R]) {
        s_key_down[KEY_STRAFE_R] = 0;
        queue_key_event(0, KEY_STRAFE_R);
      } else if (k == KEY_STRAFE_R && s_key_down[KEY_STRAFE_L]) {
        s_key_down[KEY_STRAFE_L] = 0;
        queue_key_event(0, KEY_STRAFE_L);
      }

      if (!s_key_down[k]) {
        s_key_down[k] = 1;
        queue_key_event(1, k);
      }
      s_key_press_time[k] = now;
    }
  }

  for (int i = 0; i < 256; i++) {
    if (s_key_down[i]) {
      if ((now - s_key_press_time[i]) > KEY_RELEASE_TIMEOUT_MS) {
        s_key_down[i] = 0;
        queue_key_event(0, (unsigned char)i);
      }
    }
  }
}

int DG_GetKey(int *pressed, unsigned char *doomKey) {
  if (kbd_fd >= 0) {
    unsigned char ev[2];
    if (read(kbd_fd, ev, 2) == 2) {
      *pressed = ev[0];
      *doomKey = convert_char_to_doom(ev[1]);
      return 1;
    }
  }

  poll_keys();

  if (s_eq_tail != s_eq_head) {
    *pressed = s_events[s_eq_tail].pressed;
    *doomKey = s_events[s_eq_tail].key;
    s_eq_tail = (s_eq_tail + 1) % EVENT_QUEUE_SIZE;
    return 1;
  }

  return 0;
}

int DG_GetMouse(int *buttons, int *xrel, int *yrel) {
  if (mouse_fd < 0) return 0;
  signed char ev[4];
  if (read(mouse_fd, ev, 4) == 4) {
    *buttons = (unsigned char)ev[0];
    *xrel = (int)ev[1];
    *yrel = (int)ev[2];
    return 1;
  }
  return 0;
}

void DG_SetWindowTitle(const char *title) {
  (void)title;
}

#include "i_sound.h"
#include "w_wad.h"
#include "z_zone.h"
#include "m_misc.h"
#include "deh_str.h"

static int dsp_fd = -1;

int use_libsamplerate = 0;
float libsamplerate_scale = 1.0f;

#define NUM_CHANNELS 8
typedef struct {
  const unsigned char *data;
  int len;
  uint32_t pos_fp;
  uint32_t step_fp;
  int vol_l;
  int vol_r;
  int active;
} channel_t;

static channel_t channels[NUM_CHANNELS];

#define SAMPLES_PER_TIC 1260
static int16_t mix_buffer[SAMPLES_PER_TIC * 2];

static snddevice_t sound_beanie_devices[] = {
  SNDDEVICE_SB,
  SNDDEVICE_PAS,
  SNDDEVICE_GUS,
};

static boolean Sound_Init(boolean use_sfx_prefix) {
  (void)use_sfx_prefix;
  dsp_fd = open("/dev/dsp", O_WRONLY);
  printf("Doom: Sound_Init: /dev/dsp opened, dsp_fd=%d\n", dsp_fd);
  for (int i = 0; i < NUM_CHANNELS; i++) {
    channels[i].active = 0;
  }
  return (dsp_fd >= 0);
}

static void Sound_Shutdown(void) {
  if (dsp_fd >= 0) {
    close(dsp_fd);
    dsp_fd = -1;
  }
}

static int Sound_GetSfxLumpNum(sfxinfo_t *sfx) {
  char namebuf[9];
  if (sfx->link != NULL) {
    sfx = sfx->link;
  }
  M_snprintf(namebuf, sizeof(namebuf), "ds%s", DEH_String(sfx->name));
  return W_CheckNumForName(namebuf);
}

static void Sound_UpdateSoundParams(int channel, int vol, int sep) {
  if (channel < 0 || channel >= NUM_CHANNELS) return;
  channels[channel].vol_l = ((254 - sep) * vol) / 127;
  channels[channel].vol_r = (sep * vol) / 127;
}

static int Sound_StartSound(sfxinfo_t *sfx, int channel, int vol, int sep) {
  if (channel < 0 || channel >= NUM_CHANNELS) {
    channel = -1;
    for (int i = 0; i < NUM_CHANNELS; i++) {
      if (!channels[i].active) {
        channel = i;
        break;
      }
    }
    if (channel < 0) channel = 0;
  }

  if (sfx->lumpnum < 0) {
    sfx->lumpnum = Sound_GetSfxLumpNum(sfx);
    if (sfx->lumpnum < 0) return -1;
  }

  int lumplen = W_LumpLength(sfx->lumpnum);
  if (lumplen < 8) return -1;

  const unsigned char *raw = (const unsigned char *)W_CacheLumpNum(sfx->lumpnum, PU_STATIC);
  if (!raw) return -1;

  int sample_count = (int)(raw[4] | (raw[5] << 8) | (raw[6] << 16) | (raw[7] << 24));
  if (sample_count <= 0) return -1;
  if (sample_count > lumplen - 8) sample_count = lumplen - 8;

  uint16_t rate = (uint16_t)(raw[2] | (raw[3] << 8));
  if (rate == 0) rate = 11025;

  channels[channel].data = raw + 8;
  channels[channel].len = sample_count;
  channels[channel].pos_fp = 0;
  channels[channel].step_fp = ((uint32_t)rate << 16) / 44100;
  Sound_UpdateSoundParams(channel, vol, sep);
  channels[channel].active = 1;

  return channel;
}

static void Sound_StopSound(int channel) {
  if (channel >= 0 && channel < NUM_CHANNELS) {
    channels[channel].active = 0;
  }
}

static boolean Sound_SoundIsPlaying(int channel) {
  if (channel >= 0 && channel < NUM_CHANNELS) {
    return channels[channel].active;
  }
  return false;
}

static void Sound_CacheSounds(sfxinfo_t *sounds, int num_sounds) {
  (void)sounds;
  (void)num_sounds;
}

static int silence_frames = 0;

static void Sound_Update(void) {
  if (dsp_fd < 0) return;

  memset(mix_buffer, 0, sizeof(mix_buffer));
  int any_active = 0;

  for (int ch = 0; ch < NUM_CHANNELS; ch++) {
    if (!channels[ch].active) continue;
    any_active = 1;

    int vol_l = channels[ch].vol_l;
    int vol_r = channels[ch].vol_r;
    const unsigned char *data = channels[ch].data;
    int len = channels[ch].len;
    uint32_t pos_fp = channels[ch].pos_fp;
    uint32_t step_fp = channels[ch].step_fp;

    for (int s = 0; s < SAMPLES_PER_TIC; s++) {
      int src_idx = (int)(pos_fp >> 16);
      if (src_idx >= len) {
        channels[ch].active = 0;
        break;
      }
      int sample = ((int)data[src_idx] - 128) * 256;
      int left_val  = (sample * vol_l) / 127;
      int right_val = (sample * vol_r) / 127;

      int l = mix_buffer[s * 2] + left_val;
      int r = mix_buffer[s * 2 + 1] + right_val;

      mix_buffer[s * 2]     = l > 32767 ? 32767 : (l < -32768 ? -32768 : (int16_t)l);
      mix_buffer[s * 2 + 1] = r > 32767 ? 32767 : (r < -32768 ? -32768 : (int16_t)r);

      pos_fp += step_fp;
    }

    channels[ch].pos_fp = pos_fp;
  }

  if (any_active) {
    silence_frames = 4;
    write(dsp_fd, mix_buffer, sizeof(mix_buffer));
  } else if (silence_frames > 0) {
    silence_frames--;
    write(dsp_fd, mix_buffer, sizeof(mix_buffer));
  }
}

sound_module_t DG_sound_module = {
  sound_beanie_devices,
  sizeof(sound_beanie_devices) / sizeof(sound_beanie_devices[0]),
  Sound_Init,
  Sound_Shutdown,
  Sound_GetSfxLumpNum,
  Sound_Update,
  Sound_UpdateSoundParams,
  Sound_StartSound,
  Sound_StopSound,
  Sound_SoundIsPlaying,
  Sound_CacheSounds,
};

static snddevice_t music_beanie_devices[] = {
  SNDDEVICE_SB,
  SNDDEVICE_ADLIB,
  SNDDEVICE_GENMIDI,
};

static boolean Music_Init(void) { return true; }
static void Music_Shutdown(void) {}
static void Music_SetMusicVolume(int v) { (void)v; }
static void Music_PauseMusic(void) {}
static void Music_ResumeMusic(void) {}
static void *Music_RegisterSong(void *data, int len) { (void)data; (void)len; return (void*)1; }
static void Music_UnRegisterSong(void *handle) { (void)handle; }
static void Music_PlaySong(void *handle, boolean looping) { (void)handle; (void)looping; }
static void Music_StopSong(void) {}
static boolean Music_MusicIsPlaying(void) { return false; }
static void Music_Poll(void) {}

music_module_t DG_music_module = {
  music_beanie_devices,
  sizeof(music_beanie_devices) / sizeof(music_beanie_devices[0]),
  Music_Init,
  Music_Shutdown,
  Music_SetMusicVolume,
  Music_PauseMusic,
  Music_ResumeMusic,
  Music_RegisterSong,
  Music_UnRegisterSong,
  Music_PlaySong,
  Music_StopSong,
  Music_MusicIsPlaying,
  Music_Poll,
};

int main(int argc, char **argv) {
  doomgeneric_Create(argc, argv);
  while (1) {
    doomgeneric_Tick();
  }
  return 0;
}