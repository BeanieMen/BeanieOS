[org 0x7c00]
[bits 16]

; Save BIOS boot drive
mov [BOOT_DRIVE], dl


; ES:BX = 0000:1000 (kernel load address)
xor ax, ax
mov es, ax
mov bx, 0x1000

; Read 5 sectors starting at sector 2
mov ah, 0x02
mov al, 5
mov ch, 0
mov cl, 2
mov dh, 0
mov dl, [BOOT_DRIVE]
int 0x13

; Disable interrupts before mode switch
cli

; Load GDT address into GDTR
lgdt [gdt_descriptor]

; Set PE bit in CR0
mov eax, cr0
or eax, 1
mov cr0, eax

; Reload CS using the new gdt code segment descriptor
jmp CODE_SEG:init_pm

BOOT_DRIVE db 0

; -------------------------
; GDT
; -------------------------

gdt_start:

; Null descriptor (for intel idk why)
gdt_null:
    dw 0x0000
    dw 0x0000
    dw 0x0000
    dw 0x0000

; Code segment:
; Base=0 Limit=4GB Execute+Read 32-bit
gdt_code:
    dw 0xFFFF      ; limit low
    dw 0x0000      ; base low
    db 0x00        ; base mid
    db 0x9A        ; access
    db 0xCF        ; flags + limit high
    db 0x00        ; base high

; Data segment:
; Base=0 Limit=4GB Read+Write 32-bit
gdt_data:
    dw 0xFFFF
    dw 0x0000
    db 0x00
    db 0x92
    db 0xCF
    db 0x00

gdt_end:

; GDTR structure
gdt_descriptor:
    dw gdt_end - gdt_start - 1
    dd gdt_start

; Segment selectors
CODE_SEG equ gdt_code - gdt_start
DATA_SEG equ gdt_data - gdt_start

[bits 32]

init_pm:

    ; Load data segment selector
    mov ax, DATA_SEG
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    ; Set stack
    mov esp, 0x90000

    ; Jump to loaded kernel
    jmp 0x1000

times 510-($-$$) db 0
dw 0xaa55