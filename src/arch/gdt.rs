use spin::Once;
use x86_64::VirtAddr;
use x86_64::structures::gdt::SegmentSelector;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;
pub(crate) const DOUBLE_FAULT_IST_INDEX: u16 = 0;

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
        let user_code_selector = gdt.append(Descriptor::user_code_segment());
        let user_data_selector = gdt.append(Descriptor::user_data_segment());
        (
            gdt,
            Selectors {
                code_selector,
                tss_selector,
                user_code_selector,
                user_data_selector,
            },
        )
    })
}

struct Selectors {
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    user_code_selector: SegmentSelector,
    user_data_selector: SegmentSelector,
}

pub(crate) fn user_code_selector() -> SegmentSelector {
    gdt().1.user_code_selector
}

pub(crate) fn user_data_selector() -> SegmentSelector {
    gdt().1.user_data_selector
}

pub(crate) fn kernel_code_selector() -> SegmentSelector {
    gdt().1.code_selector
}

pub(crate) fn tss_addr() -> u64 {
    tss() as *const TaskStateSegment as u64
}

pub(crate) fn gdt_addr() -> u64 {
    &raw const gdt().0 as u64
}

pub(crate) fn init() {
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
