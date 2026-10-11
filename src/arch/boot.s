.section .text
.code32

.global _start
.type _start, @function

.equ PTE_PRESENT, 0x001
.equ PTE_WRITABLE, 0x002
.equ PTE_HUGE, 0x080
.equ PDE_FLAGS, (PTE_PRESENT | PTE_WRITABLE | PTE_HUGE)

.equ PAGE_SIZE, 4096
.equ PAGE_WORDS, (PAGE_SIZE / 4)
.equ TABLE_ENTRIES, 512
.equ TABLE_ENTRY_SIZE, 8
.equ TABLE_ENTRY_HIGH, 4
.equ HUGE_PAGE_SIZE, 2097152

.equ BOOT_STACK_SIZE, 1048576
.equ BOOT_STACK_TOP, BOOT_STACK_SIZE

.equ CODE_SELECTOR, 0x08
.equ DATA_SELECTOR, 0x10

.equ MSR_EFER, 0xC0000080
.equ EFER_LME, (1 << 8)
.equ CR4_PAE, (1 << 5)
.equ CR4_OSFXSR, (1 << 9)
.equ CR4_OSXMMEXCPT, (1 << 10)
.equ CR0_MP, (1 << 1)
.equ CR0_EM, (1 << 2)
.equ CR0_PG, (1 << 31)

.equ KERNEL_P4_INDEX, 511
.equ IMAGE_P3_INDEX, 510
.equ DIRECT_MAP_P4_INDEX, 510

.equ IDENTITY_TABLES, 8
.equ DIRECT_MAP_TABLES, 4

_start:
    cli
    cld

    # Preserve Multiboot2 boot magic and information pointer.
    mov %eax, %ebp
    mov %ebx, %esi

    # Initialize the temporary physical boot stack.
    mov $(BOOT_STACK_phys + BOOT_STACK_TOP), %esp

    # Clear the complete P4 table.
    mov $P4_TABLE_phys, %edi
    xor %eax, %eax
    mov $PAGE_WORDS, %ecx
    rep stosl

    # Clear the complete higher-half P3 table.
    mov $P3_HI_phys, %edi
    xor %eax, %eax
    mov $PAGE_WORDS, %ecx
    rep stosl

    # Initialize one P2 table for the higher-half mapping.
    mov $P2_HI_phys, %edi
    mov $1, %ecx
    call fill_p2_tables

    # Initialize eight P2 tables for the identity mapping.
    mov $P2_TABLES_phys, %edi
    mov $IDENTITY_TABLES, %ecx
    call fill_p2_tables

    # Initialize four P2 tables for the direct physical map.
    mov $P2_MAP_phys, %edi
    mov $DIRECT_MAP_TABLES, %ecx
    call fill_p2_tables

    # Connect P4 entry zero to the identity-map P3 table.
    mov $P3_TABLE_phys, %eax
    or $(PTE_PRESENT | PTE_WRITABLE), %eax
    mov %eax, (P4_TABLE_phys)
    movl $0, (P4_TABLE_phys + TABLE_ENTRY_HIGH)

    # Connect each identity-map P3 entry to a P2 table.
    xor %ecx, %ecx

.wire_p3_table:
    mov %ecx, %eax
    shl $12, %eax
    add $P2_TABLES_phys, %eax
    or $(PTE_PRESENT | PTE_WRITABLE), %eax
    mov %eax, (P3_TABLE_phys)(, %ecx, TABLE_ENTRY_SIZE)
    movl $0, (P3_TABLE_phys + TABLE_ENTRY_HIGH)(, %ecx, TABLE_ENTRY_SIZE)
    inc %ecx
    cmp $IDENTITY_TABLES, %ecx
    jne .wire_p3_table

    # Connect higher-half P3 entry 510 to the higher-half P2 table.
    mov $P2_HI_phys, %eax
    or $(PTE_PRESENT | PTE_WRITABLE), %eax
    mov %eax, (P3_HI_phys + IMAGE_P3_INDEX * TABLE_ENTRY_SIZE)
    movl $0, (P3_HI_phys + IMAGE_P3_INDEX * TABLE_ENTRY_SIZE + TABLE_ENTRY_HIGH)

    # Connect P4 entry 511 to the higher-half P3 table.
    mov $P3_HI_phys, %eax
    or $(PTE_PRESENT | PTE_WRITABLE), %eax
    mov %eax, (P4_TABLE_phys + KERNEL_P4_INDEX * TABLE_ENTRY_SIZE)
    movl $0, (P4_TABLE_phys + KERNEL_P4_INDEX * TABLE_ENTRY_SIZE + TABLE_ENTRY_HIGH)

    # Connect the direct-map P3 entries to their P2 tables.
    xor %ecx, %ecx

.wire_p3_map:
    mov %ecx, %eax
    shl $12, %eax
    add $P2_MAP_phys, %eax
    or $(PTE_PRESENT | PTE_WRITABLE), %eax
    mov %eax, (P3_MAP_phys)(, %ecx, TABLE_ENTRY_SIZE)
    movl $0, (P3_MAP_phys + TABLE_ENTRY_HIGH)(, %ecx, TABLE_ENTRY_SIZE)
    inc %ecx
    cmp $DIRECT_MAP_TABLES, %ecx
    jne .wire_p3_map

    # Connect P4 entry 510 to the direct-map P3 table.
    mov $P3_MAP_phys, %eax
    or $(PTE_PRESENT | PTE_WRITABLE), %eax
    mov %eax, (P4_TABLE_phys + DIRECT_MAP_P4_INDEX * TABLE_ENTRY_SIZE)
    movl $0, (P4_TABLE_phys + DIRECT_MAP_P4_INDEX * TABLE_ENTRY_SIZE + TABLE_ENTRY_HIGH)

    # Load the physical address of the top-level P4 table.
    mov $P4_TABLE_phys, %eax
    mov %eax, %cr3

    # Enable PAE page-table structures.
    mov %cr4, %eax
    or $CR4_PAE, %eax
    mov %eax, %cr4

    # Let SSE reach the hardware. The compiler emits it in user space on its own,
    # and without these bits every such instruction raises #UD.
    mov %cr4, %eax
    or $CR4_OSFXSR, %eax
    or $CR4_OSXMMEXCPT, %eax
    mov %eax, %cr4

    mov %cr0, %eax
    mov $CR0_EM, %edx
    not %edx
    and %edx, %eax
    or $CR0_MP, %eax
    mov %eax, %cr0

    # Enable long mode through EFER.LME.
    mov $MSR_EFER, %ecx
    rdmsr
    or $EFER_LME, %eax
    wrmsr

    # Enable paging to activate IA-32e mode.
    mov %cr0, %eax
    or $CR0_PG, %eax
    mov %eax, %cr0

    # Load the GDT using its physical address.
    lgdt gdt64_pointer_phys

    # Load the 64-bit code selector and enter long mode.
    ljmp $CODE_SELECTOR, $long_mode_start_phys

.code64

.global long_mode_start
.type long_mode_start, @function

long_mode_start:
    # Initialize data segment registers.
    mov $DATA_SELECTOR, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    # Switch to the kernel's virtual boot stack.
    movabs $(BOOT_STACK + BOOT_STACK_TOP), %rsp

    # Pass the Multiboot2 magic and information pointer to Rust.
    mov %ebp, %edi
    mov %esi, %esi

    # Call the Rust kernel entry point.
    movabs $rust_entry, %rax
    call *%rax

.hang:
    # Halt permanently if the Rust entry point returns.
    cli
    hlt
    jmp .hang

.size _start, . - _start

.code32

.type fill_p2_tables, @function

fill_p2_tables:
    # Start mapping physical memory at address zero.
    xor %eax, %eax
    xor %edx, %edx

.fill_p2_table:
    # Save the number of remaining P2 tables.
    push %ecx
    mov $TABLE_ENTRIES, %ecx

.fill_p2_entry:
    # Construct a present, writable, 2 MiB page-directory entry.
    mov %eax, %ebx
    or $PDE_FLAGS, %ebx
    mov %ebx, (%edi)

    # Store the upper 32 bits of the physical address.
    mov %edx, TABLE_ENTRY_HIGH(%edi)

    # Advance the physical address by one 2 MiB page.
    add $HUGE_PAGE_SIZE, %eax
    adc $0, %edx

    # Advance to the next 64-bit page-directory entry.
    add $TABLE_ENTRY_SIZE, %edi
    dec %ecx
    jnz .fill_p2_entry

    # Continue with the next P2 table.
    pop %ecx
    dec %ecx
    jnz .fill_p2_table

    ret

.size fill_p2_tables, . - fill_p2_tables

.section .rodata
.align 8

.global gdt64
gdt64:
    # Null descriptor.
    .quad 0

    # Ring-zero executable and readable 64-bit code segment.
    .quad (1 << 41) | (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)

    # Ring-zero data segment.
    .quad (1 << 41) | (1 << 44) | (1 << 47)

gdt64_end:

.align 8

.global gdt64_pointer
gdt64_pointer:
    # GDT limit followed by its physical base address.
    .short gdt64_end - gdt64 - 1
    .long gdt64_phys

.section .multiboot_header,"a",@progbits
.align 8

.global multiboot_header
multiboot_header:
    # Multiboot2 magic, architecture, header length, and checksum.
    .long 0xE85250D6
    .long 0
    .long 80
    .long -(0xE85250D6 + 80)

    # Request memory map, framebuffer, ELF sections, command line, and ACPI data.
    .short 1
    .short 0
    .long 32
    .long 6
    .long 8
    .long 15
    .long 9
    .long 1
    .long 14

    # Request a 1024x768 framebuffer with 32 bits per pixel.
    .short 5
    .short 0
    .long 20
    .long 1024
    .long 768
    .long 32

    # Pad the framebuffer tag to an 8-byte boundary.
    .long 0

    # Required Multiboot2 end tag.
    .short 0
    .short 0
    .long 8

.section .bss

# Allocate the top-level P4 page table.
.align 4096
.global P4_TABLE
P4_TABLE:
    .zero 4096

# Allocate the identity-map P3 table.
.align 4096
.global P3_TABLE
P3_TABLE:
    .zero 4096

# Allocate the higher-half P3 table.
.align 4096
.global P3_HI
P3_HI:
    .zero 4096

# Allocate the direct-map P3 table.
.align 4096
.global P3_MAP
P3_MAP:
    .zero 4096

# Allocate one higher-half P2 table.
.align 4096
.global P2_HI
P2_HI:
    .zero 4096

# Allocate eight P2 tables for the identity map.
.align 4096
.global P2_TABLES
P2_TABLES:
    .zero 4096 * IDENTITY_TABLES

# Allocate four P2 tables for the direct map.
.align 4096
.global P2_MAP
P2_MAP:
    .zero 4096 * DIRECT_MAP_TABLES

# Reserve a 1 MiB boot stack.
.align 16
.global BOOT_STACK
BOOT_STACK:
    .zero BOOT_STACK_SIZE
