[bits 32]

global keyboard_isr
extern keyboard_handler ; get from c code

keyboard_isr:
    pusha
    cld 

    call keyboard_handler ; Call the C handler for the keyboard interrupt

    popa                ; Restore all registers
    iretd                ; Return from the interrupt
