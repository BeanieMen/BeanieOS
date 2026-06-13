#include <stdint.h>
#include "vga/vga.h"
#include "helpers/helpers.h"
#include "idt.h"
#include "pic.h"
void kmain(void)
{
    pic_remap();
    idt_install();
    vga_clear();
    
    // Initial shell prompt
    vga_print("BeanieOS 1.0\n");
    vga_print("> ");

    // infinite loop so no weird bootloop
    for (;;)
    {
        __asm__ volatile("hlt");
    }
}