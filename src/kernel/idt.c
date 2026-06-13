#include <stdint.h>
#include "io/io.h"
#include "vga/vga.h"

// IDT Entry Structure
struct idt_entry {
    uint16_t base_low;
    uint16_t sel;
    uint8_t zero;
    uint8_t flags;
    uint16_t base_high;
} __attribute__((packed));

struct idt_ptr {
    uint16_t limit;
    uint32_t base;
} __attribute__((packed));

struct idt_entry idt[256];
struct idt_ptr idtp;

extern void keyboard_isr(); // From your ASM file

void idt_set_gate(uint8_t num, uint32_t base, uint16_t sel, uint8_t flags) {
    idt[num].base_low = base & 0xFFFF;
    idt[num].base_high = (base >> 16) & 0xFFFF;
    idt[num].sel = sel;
    idt[num].zero = 0;
    idt[num].flags = flags;
}

// US Keyboard layout map
const char kbd_us[128] = {
    0,  27, '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\b',
  '\t', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n',
    0, 'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`', 0,
 '\\', 'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/', 0, '*', 0, ' ', 
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
};

// Global variables for our simple shell
static char shell_buf[256];
static int shell_buf_idx = 0;

void keyboard_handler() {
    uint8_t status = inb(0x64);
    
    if (status & 0x01) {
        // Read the scancode
        uint8_t scancode = inb(0x60);
        
        // 0x80 bit is set on key release. We only care about key presses here.
        if (!(scancode & 0x80)) {
            char c = kbd_us[scancode];
            if (c) {
                if (c == '\b') {
                    if (shell_buf_idx > 0) {
                        shell_buf_idx--;
                        vga_backspace();
                    }
                } else if (c == '\n') {
                    vga_putchar('\n');
                    
                    // Echo the buffer
                    shell_buf[shell_buf_idx] = '\0';
                    vga_print(shell_buf);
                    
                    // Reset buffer and print new prompt
                    shell_buf_idx = 0;
                    vga_print("\n> ");
                } else {
                    if (shell_buf_idx < 255) {
                        shell_buf[shell_buf_idx++] = c;
                        vga_putchar(c);
                    }
                }
            }
        }
    }
    
    outb(0x20, 0x20);
}

void idt_install() {
    idtp.limit = (sizeof(struct idt_entry) * 256) - 1;
    idtp.base = (uint32_t)&idt;

    // Set up the keyboard interrupt (IRQ1 = 32 + 1 = 33)
    // 0x08 is the Code Segment selector in your GDT
    // 0x8E means it's an Interrupt Gate (present, ring 0)
    idt_set_gate(33, (uint32_t)keyboard_isr, 0x08, 0x8E);

    __asm__ volatile("lidt %0" : : "m" (idtp));
    
    // Enable CPU interrupts
    __asm__ volatile("sti");
}