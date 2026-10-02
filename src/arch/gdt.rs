use spin::Once;
use x86_64::VirtAddr;
use x86_64::structures::gdt::SegmentSelector;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

// 20 KiB stacks
static TSS: Once<TaskStateSegment> = Once::new();

fn tss() -> &'static TaskStateSegment {
    TSS.call_once(|| {
        let mut tss = TaskStateSegment::new();

        const STACK_SIZE: usize = 4096 * 5;

        static mut DOUBLE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
        static mut KERNEL_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            let stack_start = VirtAddr::from_ptr(&raw const DOUBLE_FAULT_STACK);
            stack_start + STACK_SIZE as u64
        };

        tss.privilege_stack_table[0] = {
            let stack_start = VirtAddr::from_ptr(&raw const KERNEL_STACK);
            stack_start + STACK_SIZE as u64
        };
        tss
    })
}

static GDT: Once<(GlobalDescriptorTable, Selectors)> = Once::new();

fn gdt() -> &'static (GlobalDescriptorTable, Selectors) {
    GDT.call_once(|| {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(tss()));
        (
            gdt,
            Selectors {
                code_selector,
                tss_selector,
            },
        )
    })
}

struct Selectors {
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    let gdt = gdt();
    gdt.0.load();
    unsafe {
        CS::set_reg(gdt.1.code_selector);
        DS::set_reg(SegmentSelector(0)); // unused stuff
        ES::set_reg(SegmentSelector(0));
        FS::set_reg(SegmentSelector(0));
        GS::set_reg(SegmentSelector(0));
        SS::set_reg(SegmentSelector(0));
        load_tss(gdt.1.tss_selector);
    }
}
