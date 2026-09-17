.section .multiboot_header, "a"
.align 8
header_start:
    .long 0xe85250d6                                /* magic number */
    .long 0                                         /* architecture 0 (protected mode i386) */
    .long header_end - header_start                 /* header length */
    .long -(0xe85250d6 + 0 + (header_end - header_start)) /* checksum */

    /* End tag */
    .align 8
    .short 0                                        /* type = 0 */
    .short 0                                        /* flags = 0 */
    .long 8                                         /* size = 8 */
header_end:


.section .text

.code32
.global _start
.type _start, @function

_start:
    cli

    /* Save Multiboot2 arguments */
    mov %eax, %ebp      /* %ebp = magic */
    mov %ebx, %esi      /* %esi = mbi_addr */

    /* Set up stack pointer for 32-bit */
    mov $stack_top, %esp

    /* Set up identity paging for 0 .. 8 GiB */
    /* Map p4_table[0] -> p3_table */
    
    mov $p3_table, %eax
    or $0x3, %eax       /* Present + Writable */
    mov %eax, p4_table
    movl $0, p4_table + 4

    /* Map p3_table[0..7] -> p2_table[0..7] */
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

    /* Map 8 * 512 = 4096 entries of 2MiB pages (0 .. 8 GiB) */
    mov $0, %ecx

.map_p2_table:
    mov $0x200000, %eax /* 2MiB */
    mul %ecx            /* edx:eax = ecx * 2MiB */
    or $0x83, %eax      /* Present + Writable + Huge (2MiB) */
    mov %eax, p2_table(, %ecx, 8)
    mov %edx, p2_table + 4(, %ecx, 8)
    inc %ecx
    cmp $4096, %ecx
    jne .map_p2_table

    /* Load CR3 with address of p4_table */
    mov $p4_table, %eax
    mov %eax, %cr3

    /* Enable PAE in CR4 */
    mov %cr4, %eax
    or $(1 << 5), %eax  /* CR4.PAE = bit 5 */
    mov %eax, %cr4

    /* Enable Long Mode in EFER MSR */
    mov $0xc0000080, %ecx /* EFER MSR */
    rdmsr
    or $(1 << 8), %eax    /* EFER.LME = bit 8 */
    wrmsr

    /* Enable Paging in CR0 */
    mov %cr0, %eax
    or $(1 << 31 | 1), %eax /* CR0.PG = bit 31, CR0.PE = bit 0 */
    mov %eax, %cr0

    /* Load 64-bit GDT */
    lgdt gdt64_pointer

    /* Far jump into 64-bit mode */
    ljmp $CODE_SEG, $long_mode_start

.code64
long_mode_start:
    /* Reload data segment registers */
    mov $DATA_SEG, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    /* Set up 64-bit stack pointer */
    mov $stack_top, %rsp

    /* Pass arguments to rust_entry(magic: u32, mbi_addr: u32)
       rdi = magic (saved in ebp)
       rsi = mbi_addr (saved in esi) */
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
    .quad 0                             /* Null descriptor */
.equ CODE_SEG, . - gdt64
    .quad (1 << 41) | (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53) /* 64-bit code descriptor */
.equ DATA_SEG, . - gdt64
    .quad (1 << 41) | (1 << 44) | (1 << 47)                         /* 64-bit data descriptor */
gdt64_end:

.align 8
gdt64_pointer:
    .short gdt64_end - gdt64 - 1
    .long gdt64

.section .bss
.align 4096

/* has 1 p3 entry */
p4_table:
    .skip 4096

/* has 8 p2 entries */
p3_table:
    .skip 4096

/* has 512 pages of 2MiB */
p2_table:
    .skip 4096 * 8

.align 16
stack_bottom:
    .skip 65536
stack_top: