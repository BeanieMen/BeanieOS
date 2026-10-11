pub const PAGE_SIZE: u64 = 4096;
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
pub const MMIO_BASE: u64 = 0xFFFF_F800_0000_0000;
pub const MMIO_WINDOW: u64 = KERNEL_VIRTUAL_BASE - MMIO_BASE;

pub const HEAP_OFFSET: u64 = 1 << 30;
pub const HEAP_BASE: u64 = DIRECT_MAP_BASE + HEAP_OFFSET;

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
    assert!(phys < DIRECT_MAP_SIZE, "physical address outside the direct map");
    phys + DIRECT_MAP_BASE
}