.section .multiboot_header, "a"
.align 8

header_start:
    .long 0xe85250d6
    .long 0
    .long header_end - header_start
    .long -(0xe85250d6 + 0 + (header_end - header_start))

    /* Information request tag: ask for mmap (6), framebuffer (8),
       ELF sections (9) + cmdline (1) so the kernel can avoid
       overwriting itself. 4 entries -> size 24, 8-byte aligned. */
    .align 8
    .short 1                /* type = information request */
    .short 0                /* flags */
    .long 24                /* size = 8 + 4*4 */
    .long 6                 /* mmap */
    .long 8                 /* framebuffer */
    .long 9                 /* ELF sections */
    .long 1                 /* boot cmdline */

    /* Framebuffer tag: request a 1024x768x32 mode if possible.
       Width/height 0 would also work, but an explicit request makes
       Limine/GRUB bring up GOP/VBE reliably. Size 20 + 4 pad = 24. */
    .align 8
    .short 5                /* type = framebuffer */
    .short 0                /* flags */
    .long 20                /* size */
    .long 1024              /* width */
    .long 768               /* height */
    .long 32                /* depth */
    .long 0                 /* padding to keep next tag 8-aligned */

    .align 8
    .short 0
    .short 0
    .long 8

header_end:


.section .text

.code32
.global _start
.type _start, @function

_start:
    cli

    mov %eax, %ebp
    mov %ebx, %esi

    mov $stack_top, %esp

    mov $p3_table, %eax
    or $0x3, %eax
    mov %eax, p4_table
    movl $0, p4_table + 4

    mov $0, %ecx

.map_p3_table:
    mov $4096, %eax
    mul %ecx
    add $p2_table, %eax
    or $0x3, %eax

    mov %eax, p3_table(, %ecx, 8)
    movl $0, p3_table + 4(, %ecx, 8)

    inc %ecx
    cmp $8, %ecx
    jne .map_p3_table

    mov $0, %ecx

.map_p2_table:
    mov $0x200000, %eax
    mul %ecx

    or $0x83, %eax

    mov %eax, p2_table(, %ecx, 8)
    mov %edx, p2_table + 4(, %ecx, 8)

    inc %ecx
    cmp $4096, %ecx
    jne .map_p2_table

    mov $p4_table, %eax
    mov %eax, %cr3

    mov %cr4, %eax
    or $(1 << 5), %eax
    mov %eax, %cr4

    mov $0xc0000080, %ecx
    rdmsr
    or $(1 << 8), %eax
    wrmsr

    mov %cr0, %eax
    or $(1 << 31 | 1), %eax
    mov %eax, %cr0

    lgdt gdt64_pointer

    ljmp $CODE_SEG, $long_mode_start


.code64

long_mode_start:
    mov $DATA_SEG, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    mov $stack_top, %rsp

    mov %ebp, %edi
    mov %esi, %esi

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


.section .bss

.align 4096

p4_table:
    .skip 4096

p3_table:
    .skip 4096

p2_table:
    .skip 4096 * 8

.align 16

stack_bottom:
    .skip 65536

stack_top:
