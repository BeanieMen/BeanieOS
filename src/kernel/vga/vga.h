
#ifndef VGA_H
#define VGA_H

#include <stdint.h>

void vga_clear(void);
void vga_putchar(char c);
void vga_print(const char* str);
void vga_backspace(void);

#endif