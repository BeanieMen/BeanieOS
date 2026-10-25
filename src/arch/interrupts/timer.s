.section .text
.global timer_interrupt_entry
.type timer_interrupt_entry, @function

timer_interrupt_entry:
    cld

    push %rax
    push %rcx
    push %rdx
    push %rbx
    push %rsi
    push %rdi
    push %rbp
    push %r8
    push %r9
    push %r10
    push %r11
    push %r12
    push %r13
    push %r14
    push %r15

    # 15 registers * 8 = 120 bytes.
    # CPU interrupt frame: rip (8), cs (8), rflags (8), rsp (8), ss (8) = 40 bytes.
    # Total on stack = 160 bytes (16-byte aligned before call).
    # CS is at offset: 15*8 + 8 = 128(%rsp).
    mov 128(%rsp), %rdi
    call timer_interrupt_rust

    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %r11
    pop %r10
    pop %r9
    pop %r8
    pop %rbp
    pop %rdi
    pop %rsi
    pop %rbx
    pop %rdx
    pop %rcx
    pop %rax

    iretq

.size timer_interrupt_entry, . - timer_interrupt_entry
