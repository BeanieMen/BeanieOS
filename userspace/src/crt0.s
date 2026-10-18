.global _start
.extern main

.section .text
_start:
    xor %rbp, %rbp
    mov (%rsp), %rdi                # argc
    lea 8(%rsp),%rsi                # envp
    lea 16(%rsp, %rdi, 8), %rdx     # argv
    call main
    mov %rax, %rdi
    mov $60, %rax                   # SYS_EXIT
    int $0x80
1:
    hlt
    jmp 1b
