pub const PAGE_SIZE: u64 = 4096;
pub const LOW_1MIB: u64 = 0x10_0000;
pub const ENTRY_SIZE: u64 = 8;
pub const ENTRIES_PER_TABLE: u64 = 512;
pub const HUGE_PAGE_SIZE: u64 = 2097152;
pub const P2_HUGE_SPAN: u64 = HUGE_PAGE_SIZE * ENTRIES_PER_TABLE;

pub const KERNEL_VIRTUAL_BASE: u64 = 0xFFFF_FFFF_8000_0000;
pub const KERNEL_PHYS_BASE: u64 = 0x10_0000;

pub const KERNEL_P4_INDEX: u64 = ENTRIES_PER_TABLE - 1;
pub const IMAGE_P3_INDEX: u64 = ENTRIES_PER_TABLE - 2;
pub const DIRECT_MAP_P4_INDEX: u64 = ENTRIES_PER_TABLE - 2;

pub const IMAGE_SIZE: u64 = 1 << 30;

pub const DIRECT_MAP_BASE: u64 = 0xFFFF_FF00_0000_0000;
pub const DIRECT_MAP_SIZE: u64 = 4 << 30;
pub const DIRECT_MAP_TABLES: u64 = DIRECT_MAP_SIZE / P2_HUGE_SPAN;

pub const MMIO_P3_INDEX: u64 = 0;
pub const MMIO_BASE: u64 = 0xFFFF_FFFF_8000_0000 - (1 << 30);
pub const MMIO_WINDOW: u64 = 1 << 30;

pub const HEAP_OFFSET: u64 = 1 << 30;
pub const HEAP_BASE: u64 = DIRECT_MAP_BASE + HEAP_OFFSET;

pub const PTE_PRESENT: u64 = 1 << 0;
pub const PTE_WRITABLE: u64 = 1 << 1;
pub const PTE_USER: u64 = 1 << 2;
pub const PTE_HUGE: u64 = 1 << 7;
pub const PTE_COW: u64 = 1 << 9;

pub const PTE_LEAF_MASK: u64 = PTE_PRESENT | PTE_WRITABLE;
pub const PTE_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;
pub const PTE_PWT: u64 = 1 << 3;
pub const PTE_PCD: u64 = 1 << 4;
pub const PTE_KERNEL_RW: u64 = PTE_PRESENT | PTE_WRITABLE;
pub const PTE_USER_RW: u64 = PTE_PRESENT | PTE_WRITABLE | PTE_USER;
pub const PTE_USER_WC: u64 = PTE_PRESENT | PTE_WRITABLE | PTE_USER | PTE_PWT;

const _: () = assert!(IMAGE_P3_INDEX == 510);
const _: () = assert!(KERNEL_P4_INDEX == 511);
const _: () = assert!(DIRECT_MAP_P4_INDEX == 510);
const _: () = assert!(DIRECT_MAP_TABLES == 4);
const _: () = assert!(DIRECT_MAP_SIZE == DIRECT_MAP_TABLES * P2_HUGE_SPAN);

pub const fn in_image(virt: u64) -> bool {
    virt >= KERNEL_VIRTUAL_BASE && virt - KERNEL_VIRTUAL_BASE < IMAGE_SIZE
}

pub const fn kernel_to_phys(virt: u64) -> u64 {
    assert!(in_image(virt), "not a linked kernel address");
    virt - KERNEL_VIRTUAL_BASE
}

pub const fn in_direct_map(virt: u64) -> bool {
    virt >= DIRECT_MAP_BASE && virt - DIRECT_MAP_BASE < DIRECT_MAP_SIZE
}

pub const fn direct_to_phys(virt: u64) -> u64 {
    assert!(in_direct_map(virt), "not a direct-mapped address");
    virt - DIRECT_MAP_BASE
}

pub const fn phys_to_direct(phys: u64) -> u64 {
    assert!(
        phys < DIRECT_MAP_SIZE,
        "physical address outside the direct map"
    );
    phys + DIRECT_MAP_BASE
}
pub const fn align_up(addr: u64) -> u64 {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

pub const fn align_down(addr: u64) -> u64 {
    addr & !(PAGE_SIZE - 1)
}
