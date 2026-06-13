[bits 32]

global _start
extern kmain

_start:
    mov esp, 0x90000 ; Set up the stack
    call kmain         ; Call the kernel main function

.hang:
    hlt               ; Halt the CPU
    jmp .hang          ; Infinite loop to prevent returning from the kernel