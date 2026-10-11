ENTRY(_start)

OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)

KERNEL_PHYS_BASE = 0x100000;
KERNEL_VIRTUAL_BASE = 0xFFFFFFFF80000000;

SECTIONS {
    . = KERNEL_VIRTUAL_BASE + KERNEL_PHYS_BASE;

    kernel_start = .;

    .boot : AT(ADDR(.boot) - KERNEL_VIRTUAL_BASE) {
        KEEP(*(.multiboot_header))
        KEEP(*(.multiboot))
    }

    .text : AT(ADDR(.text) - KERNEL_VIRTUAL_BASE) {
        KEEP(*(.text.memcmp))

        *(.text .text.*)
    }

    .rodata : AT(ADDR(.rodata) - KERNEL_VIRTUAL_BASE) {
        *(.rodata .rodata.*)
    }

    .data : AT(ADDR(.data) - KERNEL_VIRTUAL_BASE) {
        *(.data .data.*)
    }

    .bss : AT(ADDR(.bss) - KERNEL_VIRTUAL_BASE) {
        *(COMMON)
        *(.bss .bss.*)
    }

    kernel_end = .;

    _start_phys = _start - KERNEL_VIRTUAL_BASE;
    gdt64_phys = gdt64 - KERNEL_VIRTUAL_BASE;
    gdt64_pointer_phys = gdt64_pointer - KERNEL_VIRTUAL_BASE;
    long_mode_start_phys = long_mode_start - KERNEL_VIRTUAL_BASE;
    P4_TABLE_phys = P4_TABLE - KERNEL_VIRTUAL_BASE;
    P3_TABLE_phys = P3_TABLE - KERNEL_VIRTUAL_BASE;
    P2_TABLES_phys = P2_TABLES - KERNEL_VIRTUAL_BASE;
    P3_HI_phys = P3_HI - KERNEL_VIRTUAL_BASE;
    P2_HI_phys = P2_HI - KERNEL_VIRTUAL_BASE;
    P3_MAP_phys = P3_MAP - KERNEL_VIRTUAL_BASE;
    P2_MAP_phys = P2_MAP - KERNEL_VIRTUAL_BASE;

    BOOT_STACK_phys = BOOT_STACK - KERNEL_VIRTUAL_BASE;

    /DISCARD/ : {
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.comment)
    }
}