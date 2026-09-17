ENTRY(_start)

SECTIONS {
    . = 1M;

    kernel_start = .;

    .boot : {
        KEEP(*(.multiboot_header))
        KEEP(*(.multiboot))
    }

    .text : {
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss : {
        *(COMMON)
        *(.bss .bss.*)
    }

    kernel_end = .;
}
