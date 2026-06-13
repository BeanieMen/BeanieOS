#include "io/io.h"

#define PIC1_COMMAND 0x20
#define PIC1_DATA    0x21
#define PIC2_COMMAND 0xA0
#define PIC2_DATA    0xA1

void pic_remap() {
    // Start initialization sequence (in cascade mode)
    outb(PIC1_COMMAND, 0x11);
    outb(PIC2_COMMAND, 0x11);

    // Give PICs their new offsets (Master=0x20, Slave=0x28)
    outb(PIC1_DATA, 0x20);
    outb(PIC2_DATA, 0x28);

    // Tell Master there is a slave PIC at IRQ2
    outb(PIC1_DATA, 0x04);
    // Tell Slave its cascade identity
    outb(PIC2_DATA, 0x02);

    // 8086/88 (MCS-80/85) mode
    outb(PIC1_DATA, 0x01);
    outb(PIC2_DATA, 0x01);

    // Unmask only the keyboard (IRQ1) and mask everything else on PIC1
    // 0xFD = 1111 1101 (Bit 1 is 0, which unmasks IRQ1)
    outb(PIC1_DATA, 0xFD); 
    
    // Mask all interrupts on Slave PIC (we aren't using them yet)
    outb(PIC2_DATA, 0xFF);
}