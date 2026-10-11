TARGET AND BUILD
x86_64 long mode, no_std/no_main, nightly, edition 2024
custom target x86_64.json, -Zbuild-std=core,alloc
panic=abort, no unwinder
ld.lld via linker.ld, entry _start

BOOT PATH
Limine loads a Multiboot2 image from a FAT32 part at 32 MiB
boot.s runs in .code32, builds page tables, enters long mode
order: CR3, CR4.PAE, EFER.LME, CR0.PG, lgdt, ljmp $0x08
ljmp -> long_mode_start, then call *rust_entry
magic + MBI pointer passed EBP/ESI -> EDI/ESI

MEMORY LAYOUT
kernel vaddr 0xFFFF_FFFF_8000_0000, LMA 0x100000
sections use AT(ADDR - KERNEL_VIRTUAL_BASE)
page tables in .bss, zeroed by boot.s
4-level, 512 entries/table, 4 KiB and 2 MiB pages only
identity: P4[0] -> P3_TABLE -> 8xP2 -> 8 GiB
higher half: P4[511] -> P3_HI[510] -> P2_HI -> 1 GiB
direct map: P4[510] -> P3_MAP -> 4xP2 -> 4 GiB at 0xFFFF_FF00_0000_0000
heap planned at direct map + 1 GiB, allocator not wired yet

CONCURRENCY
InterruptMutex disables interrupts before testing the flag
guard restores prior interrupt state on drop
only user is the framebuffer WRITER
no IDT, no sti, so interrupts stay masked after boot