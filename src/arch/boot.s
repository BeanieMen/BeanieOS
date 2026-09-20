# 32 bit entru point just to long jump to 64 bit rust kernel

.section .text

.code32
.global _start
.type _start, @function

_start:
    cli
    cld

    mov %eax, %ebp              # multiboot magic -> callee-saved for later
    mov %ebx, %esi              # multiboot info addr -> later 2nd arg

    mov $BOOT_STACK+65536, %esp

    # P4/P3 live in .bss link P4[0] -> P3 and P3[i] -> P2_TABLES[i].
    # (P2 entries are identity addresses fully initialized by Rust.)

    mov $P4_TABLE, %edi
    xor %eax, %eax
    mov $1024, %ecx
    rep stosl                   # zero P4 (4 KiB)

    mov $P3_TABLE, %edi
    mov $1024, %ecx
    rep stosl                   # zero P3 (4 KiB)

    mov $P3_TABLE, %eax
    or $0x3, %eax               # present | writable
    mov %eax, P4_TABLE
    movl $0, P4_TABLE + 4

    mov $0, %ecx

.map_p3_table:
    mov $4096, %eax
    mul %ecx
    add $P2_TABLES, %eax
    or $0x3, %eax               # present | writable

    mov %eax, P3_TABLE(, %ecx, 8)
    movl $0, P3_TABLE + 4(, %ecx, 8)

    inc %ecx
    cmp $8, %ecx
    jne .map_p3_table

    mov $P4_TABLE, %eax
    mov %eax, %cr3

    mov %cr4, %eax
    or $(1 << 5), %eax          # PAE
    mov %eax, %cr4

    mov $0xc0000080, %ecx       # EFER
    rdmsr
    or $(1 << 8), %eax          # LME
    wrmsr

    mov %cr0, %eax
    or $(1 << 31 | 1), %eax     # paging + protected mode
    mov %eax, %cr0

    lgdt gdt64_pointer

    ljmp $0x08, $long_mode_start


.code64

long_mode_start:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    mov $BOOT_STACK+65536, %rsp

    mov %ebp, %edi              # magic -> 1st arg (zero-extends)
    mov %esi, %esi              # mbi addr -> 2nd arg (zero-extends)

    call rust_entry

.hang:
    cli
    hlt
    jmp .hang

.size _start, . - _start


.section .rodata

.align 8

gdt64:
    .quad 0

.equ CODE_SEG, . - gdt64
    .quad (1 << 41) | (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

.equ DATA_SEG, . - gdt64
    .quad (1 << 41) | (1 << 44) | (1 << 47)

gdt64_end:

.align 8

gdt64_pointer:
    .short gdt64_end - gdt64 - 1
    .long gdt64
