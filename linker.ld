ENTRY(_start)

SECTIONS {
    . = 1M;

    kernel_start = .;

    .boot : {
        KEEP(*(.multiboot_header))
        KEEP(*(.multiboot))
    }

    .text : {
        /* The C ABI builtins are an interface, not dead code. rust-lld runs
         * with --gc-sections, and memcmp in particular has no caller inside the
         * kernel -- core compares slices, so nothing here ever asks for it --
         * which drops it from the binary and breaks any future userland that
         * links against libc's declaration. Keep all four by name. */
        KEEP(*(.text.memcpy))
        KEEP(*(.text.memmove))
        KEEP(*(.text.memset))
        KEEP(*(.text.memcmp))

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
