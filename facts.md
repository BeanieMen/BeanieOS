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


EXAMPLE: PHYSICAL VS VIRTUAL -- demo_direct_map() in main.rs
alloc_frame() hands back a *physical* address. phys_to_direct() is the only
  way to turn that into something dereferenceable.
  phys  0x00123450
  virt  0xFFFFFF012345000 ... differ by exactly DIRECT_MAP_BASE
writes a magic value through the virtual pointer and reads it back, so the
  claim is checked rather than asserted
two windows exist, and they are NOT interchangeable:
  phys_to_direct   phys -> DIRECT_MAP_BASE + phys   (0xFFFFFF0000000000+)
                   for frames the pool hands out; P4[510]
  phys_to_image    phys -> KERNEL_VIRTUAL_BASE + phys
                   only valid for addresses the linker produced; P4[511]
rule that keeps this from silently breaking: dereference via phys_to_direct,
  navigate via physical. The identity map (P4[0] -> low 8 GiB) is still wired
  during boot, so a missing conversion will NOT fault there -- it will appear to
  work. Removing that identity map is what actually tests the discipline.

EXAMPLE: PHYSICAL VS VIRTUAL (demo_direct_map in main.rs)
alloc_frame() returns physical; phys_to_direct() turns it into a pointer
  phys 0x12345000 -> virt 0xFFFFFF012345000 (differ by DIRECT_MAP_BASE)
kernel image uses a *different* window: KERNEL_VIRTUAL_BASE + phys
  phys 0x12345000 -> virt 0xFFFFFFFF8012345000 (that is kernel_to_phys, not
  phys_to_direct -- they are two different windows, not one)
the write/read-back in the demo is the actual proof: if the direct map were
  wrong, the store faults instead of printing, rather than silently reading RAM
through the identity map (P4[0] still covers the low 8 GiB during boot, so a
  missing conversion would *appear* to work until that map is removed)