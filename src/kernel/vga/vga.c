#include "vga.h"

#define VGA_WIDTH 80
#define VGA_HEIGHT 25

static uint16_t *const VGA_MEMORY = (uint16_t *)0xB8000;

static int cursor_x = 0;
static int cursor_y = 0;

static uint8_t color = 0x0F; // white on black

static inline uint16_t vga_entry(char c, uint8_t color)
{
    return (color << 8) | c;
}


static void vga_scroll(void)
{
    // Move rows 1-24 to rows 0-23
    for (int y = 1; y < VGA_HEIGHT; y++)
    {
        for (int x = 0; x < VGA_WIDTH; x++)
        {
            VGA_MEMORY[(y - 1) * VGA_WIDTH + x] =
                VGA_MEMORY[y * VGA_WIDTH + x];
        }
    }

    // Clear last row
    for (int x = 0; x < VGA_WIDTH; x++)
    {
        VGA_MEMORY[(VGA_HEIGHT - 1) * VGA_WIDTH + x] =
            vga_entry(' ', color);
    }

    cursor_y = VGA_HEIGHT - 1;
}

void vga_clear(void)
{
    for (int y = 0; y < VGA_HEIGHT; y++)
    {
        for (int x = 0; x < VGA_WIDTH; x++)
        {
            VGA_MEMORY[y * VGA_WIDTH + x] = vga_entry(' ', color);
        }
    }
    cursor_x = 0;
    cursor_y = 0;
}

void vga_backspace(void)
{
    if (cursor_x > 0)
    {
        cursor_x--;
        VGA_MEMORY[cursor_y * VGA_WIDTH + cursor_x] = vga_entry(' ', color);
    }
}

void vga_putchar(char c)
{
    if (c == '\n')
    {
        cursor_x = 0;
        cursor_y++;
    }
    else
    {
        VGA_MEMORY[cursor_y * VGA_WIDTH + cursor_x] =
            vga_entry(c, color);

        cursor_x++;

        if (cursor_x >= VGA_WIDTH)
        {
            cursor_x = 0;
            cursor_y++;
        }
    }

    if (cursor_y >= VGA_HEIGHT)
    {
        vga_scroll();
    }
}

void vga_print(const char *str)
{

    while (*str)
    {
        vga_putchar(*str++);
    }
}