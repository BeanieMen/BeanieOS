.global _start
.extern main

.section .text
_start:
    # 16-byte stack alignment per AMD64 ABI
    xor %rbp, %rbp
    call main
    mov %rax, %rdi
    # exit(code) via int 0x80
    mov $0, %rax
    int $0x80
1:
    hlt
    jmp 1b