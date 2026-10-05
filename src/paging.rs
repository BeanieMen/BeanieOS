pub const PAGE_SIZE: u64 = 4096;
pub const ENTRY_SIZE: u64 = 8;
pub const ENTRIES_PER_TABLE: u64 = 512;
pub const LOW_1MIB: u64 = 0x10_0000;

pub const PTE_PRESENT: u64 = 1 << 0;
pub const PTE_HUGE: u64 = 1 << 7;

pub const PTE_WRITABLE: u64 = 1 << 1;
pub const PTE_USER: u64 = 1 << 2;

pub const PTE_LEAF_MASK: u64 = PTE_PRESENT | PTE_WRITABLE;
pub const PTE_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;
pub const PTE_KERNEL_RW: u64 = PTE_PRESENT | PTE_WRITABLE;
pub const PTE_USER_RW: u64 = PTE_PRESENT | PTE_WRITABLE | PTE_USER;
pub const PTE_BOOT_IDENTITY: u64 = PTE_PRESENT | PTE_WRITABLE | PTE_HUGE;

pub const fn align_up(addr: u64) -> u64 {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

pub const fn align_down(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}
