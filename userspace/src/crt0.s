.global _start
.extern main

.section .text
_start:
    xor %rbp, %rbp
    call main
    mov %rax, %rdi
    mov $60, %rax
    int $0x80
1:
    hlt
    jmp 1b
