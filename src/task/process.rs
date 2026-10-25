use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};

use x86_64::structures::paging::PhysFrame;

use crate::{
    arch::lock::InterruptMutex, memory::{allocator::alloc_frame, mmu::{self, free_user_tables}}, paging::phys_to_direct, storage::vfs::fd::FdTable, task::{
        identity::{self, ProcessId, ThreadId},
        signal::SigAction,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessStatus {
    Running,
    Zombie(i32),
}

pub(crate) struct Process {
    pub pid: ProcessId,
    pub ppid: Option<ProcessId>,
    pub pgid: ProcessId,
    pub pending_signals: u64,
    pub blocked_signals: u64,
    pub sigactions: [SigAction; 64],
    pub name: String,
    pub status: ProcessStatus,
    pub cwd: String,
    pub threads: Vec<ThreadId>,
    pub exit_code: Option<i32>,
    pub fds: FdTable,
    pub space: Option<PhysFrame>,
    pub heap_start: u64,
    pub heap_break: u64,
    pub mmap_next: u64,
}

pub const DEFAULT_HEAP_BASE: u64 = 0x1000_0000;
pub const DEFAULT_MMAP_BASE: u64 = 0x7000_0000_0000;

impl Drop for Process {
    fn drop(&mut self) {
        if let Some(root) = self.space.take() {
            let space = mmu::AddressSpace::from_root(root);
            drop(space);
        }
    }
}

impl Process {
    fn boot() -> Self {
        Process {
            pid: ProcessId::KERNEL,
            ppid: None,
            pgid: ProcessId::KERNEL,
            pending_signals: 0,
            blocked_signals: 0,
            sigactions: [SigAction::default(); 64],
            name: String::from("kernel"),
            status: ProcessStatus::Running,
            cwd: String::from("/"),
            threads: Vec::new(),
            exit_code: None,
            fds: FdTable::new(),
            space: None,
            heap_start: DEFAULT_HEAP_BASE,
            heap_break: DEFAULT_HEAP_BASE,
            mmap_next: DEFAULT_MMAP_BASE,
        }
    }

    fn new(name: &str, ppid: Option<ProcessId>, cwd: &str) -> Self {
        let pid = ProcessId::new();
        Process {
            pid,
            ppid,
            pgid: ppid.unwrap_or(pid),
            pending_signals: 0,
            blocked_signals: 0,
            sigactions: [SigAction::default(); 64],
            name: String::from(name),
            status: ProcessStatus::Running,
            cwd: String::from(cwd),
            threads: Vec::new(),
            exit_code: None,
            fds: FdTable::new(),
            space: None,
            heap_start: DEFAULT_HEAP_BASE,
            heap_break: DEFAULT_HEAP_BASE,
            mmap_next: DEFAULT_MMAP_BASE,
        }
    }

    pub(crate) fn attach_thread(&mut self, id: ThreadId) {
        if !self.threads.contains(&id) {
            self.threads.push(id);
        }
    }
}

struct ProcessManager {
    processes: BTreeMap<ProcessId, Arc<InterruptMutex<Process>>>,
    initialized: bool,
}

impl ProcessManager {
    const fn new() -> Self {
        ProcessManager {
            processes: BTreeMap::new(),
            initialized: false,
        }
    }

    pub(crate) fn init(&mut self) {
        if self.initialized {
            return;
        }

        let kernel = Arc::new(InterruptMutex::new(Process::boot()));

        self.processes.insert(ProcessId::KERNEL, kernel);
        self.initialized = true;
    }

    pub(crate) fn create(&mut self, name: &str, cwd: &str) -> ProcessId {
        if !self.initialized {
            self.init();
        }

        let process = Process::new(name, Some(identity::current_pid()), cwd);
        let pid = process.pid;

        self.processes
            .insert(pid, Arc::new(InterruptMutex::new(process)));

        pid
    }

    pub(crate) fn create_child(&mut self, parent_pid: ProcessId) -> Option<ProcessId> {
        let parent = self.processes.get(&parent_pid)?;
        let (
            parent_name,
            parent_cwd,
            parent_fds,
            parent_heap_start,
            parent_heap_break,
            parent_mmap_next,
            parent_pgid,
            parent_blocked_signals,
            parent_sigactions,
        ) = {
            let p = parent.lock();
            (
                p.name.clone(),
                p.cwd.clone(),
                p.fds.clone(),
                p.heap_start,
                p.heap_break,
                p.mmap_next,
                p.pgid,
                p.blocked_signals,
                p.sigactions,
            )
        };

        let mut child = Process::new(
            &alloc::format!("{}-child", parent_name),
            Some(parent_pid),
            &parent_cwd,
        );
        child.fds = parent_fds.clone();
        child.heap_start = parent_heap_start;
        child.heap_break = parent_heap_break;
        child.mmap_next = parent_mmap_next;
        child.pgid = parent_pgid;
        child.blocked_signals = parent_blocked_signals;
        child.sigactions = parent_sigactions;
        let child_pid = child.pid;
        self.processes
            .insert(child_pid, Arc::new(InterruptMutex::new(child)));
        Some(child_pid)
    }

    pub(crate) fn mmap(
        &mut self,
        pid: ProcessId,
        addr: u64,
        len: u64,
        _prot: u64,
        flags: u64,
    ) -> Result<u64, ()> {
        if len == 0 {
            return Err(());
        }

        let process = self.processes.get_mut(&pid).ok_or(())?;
        let mut p = process.lock();

        let root = p.space.ok_or(())?;
        let mut space = mmu::AddressSpace::from_root(root);

        const MAP_FIXED: u64 = 0x10;
        let target = if flags & MAP_FIXED != 0 && addr != 0 {
            addr & !4095
        } else {
            let next = p.mmap_next;
            p.mmap_next += (len + 4095) & !4095;
            next
        };

        let span = (len + 4095) & !4095;
        if target == 0 || target.checked_add(span).is_none_or(|end| end > mmu::USER_END) {
            core::mem::forget(space);
            return Err(());
        }

        let pages = len.div_ceil(4096);
        let mut failed = false;
        for i in 0..pages {
            let virt = target + i * 4096;
            let Some(frame) = alloc_frame() else {
                failed = true;
                break;
            };
            let direct = phys_to_direct(frame.start_address().as_u64()) as *mut u8;
            unsafe {
                core::ptr::write_bytes(direct, 0, 4096);
            }
            if space.map(virt, frame, mmu::USER_RW).is_err() {
                failed = true;
                break;
            }
        }
        core::mem::forget(space);
        if failed {
            return Err(());
        }
        Ok(target)
    }

    pub(crate) fn mmap_phys(
        &mut self,
        pid: ProcessId,
        phys_start: u64,
        len: u64,
    ) -> Result<u64, ()> {
        if len == 0 {
            return Err(());
        }

        let process = self.processes.get_mut(&pid).ok_or(())?;

        let mut p = process.lock();
        let root = p.space.ok_or(())?;
        let mut space = mmu::AddressSpace::from_root(root);
        let target = p.mmap_next;
        p.mmap_next += (len + 4095) & !4095;

        let span = (len + 4095) & !4095;
        if target == 0 || target.checked_add(span).is_none_or(|end| end > mmu::USER_END) {
            core::mem::forget(space);
            return Err(());
        }

        let pages = len.div_ceil(4096);
        let mut failed = false;
        for i in 0..pages {
            let virt = target + i * 4096;
            let phys_addr = phys_start + i * 4096;
            let frame = PhysFrame::containing_address(x86_64::PhysAddr::new(phys_addr));
            if space.map(virt, frame, mmu::USER_WC).is_err() {
                failed = true;
                break;
            }
        }
        core::mem::forget(space);
        if failed {
            return Err(());
        }
        Ok(target)
    }
    pub(crate) fn update_brk(&mut self, pid: ProcessId, new_brk: u64) -> Result<u64, ()> {
        let process = self.processes.get_mut(&pid).ok_or(())?;
        let mut p = process.lock();

        if new_brk == 0 || new_brk < p.heap_start {
            return Ok(p.heap_break);
        }

        let root = p.space.ok_or(())?;
        let mut space = mmu::AddressSpace::from_root(root);

        let old_page = (p.heap_break + 4095) & !4095;
        let new_page = (new_brk + 4095) & !4095;

        if new_page >= mmu::USER_END || new_page >= p.mmap_next {
            return Ok(p.heap_break);
        }

        if new_page > old_page {
            let mut curr = old_page;
            while curr < new_page {
                let Some(frame) = alloc_frame() else {
                    core::mem::forget(space);
                    return Err(());
                };
                let direct = phys_to_direct(frame.start_address().as_u64()) as *mut u8;

                unsafe {
                    core::ptr::write_bytes(direct, 0, 4096);
                }

                if space.map(curr, frame, mmu::USER_RW).is_err() {
                    core::mem::forget(space);
                    return Err(());
                }

                curr += 4096;
            }
        } else if new_page < old_page {
            let mut curr = new_page;
            while curr < old_page {
                let _ = space.unmap(curr);
                curr += 4096;
            }
        }
        core::mem::forget(space);
        p.heap_break = new_brk;
        Ok(p.heap_break)
    }
    pub(crate) fn get(&self, pid: ProcessId) -> Option<Arc<InterruptMutex<Process>>> {
        self.processes.get(&pid).cloned()
    }

    fn parent_of(&self, pid: ProcessId) -> Option<ProcessId> {
        self.processes
            .get(&pid)
            .map(|process| process.lock().ppid)?
    }

    pub(crate) fn exit(&mut self, pid: ProcessId, exit_code: i32) {
        let Some(process) = self.processes.get(&pid) else {
            return;
        };

        let mut process = process.lock();

        process.status = ProcessStatus::Zombie(exit_code);
        process.exit_code = Some(exit_code);
        process.fds.close_all();

        // Reparent orphans to init (PID 1)
        for proc in self.processes.values() {
            let mut p = proc.lock();
            if p.ppid == Some(pid) {
                p.ppid = Some(ProcessId(1));
            }
        }
    }

    pub(crate) fn reap(&mut self, pid: ProcessId) -> Option<i32> {
        let process = self.processes.get(&pid).cloned()?;

        let code = match process.lock().status {
            ProcessStatus::Zombie(code) => code,
            ProcessStatus::Running => return None,
        };

        self.processes.remove(&pid);
        if let Some(space_frame) = process.lock().space {
                    unsafe { free_user_tables(space_frame) };
            }
        Some(code)
    }
}

static PROCESS_MANAGER: InterruptMutex<ProcessManager> = InterruptMutex::new(ProcessManager::new());

pub(crate) fn init() {
    PROCESS_MANAGER.lock().init();
}

pub(crate) fn create(name: &str, cwd: &str) -> ProcessId {
    PROCESS_MANAGER.lock().create(name, cwd)
}

pub(crate) fn exit(pid: ProcessId, code: i32) {
    PROCESS_MANAGER.lock().exit(pid, code);
}

pub(crate) fn create_child(parent_pid: ProcessId) -> Option<ProcessId> {
    PROCESS_MANAGER.lock().create_child(parent_pid)
}

pub(crate) fn space_root_of(pid: ProcessId) -> Option<PhysFrame> {
    PROCESS_MANAGER.lock().get(pid)?.lock().space
}

pub(crate) fn wait4(target: i64, options: u64) -> Result<Option<(ProcessId, i32)>, ()> {
    let current = identity::current_pid();
    let wnohang = (options & 1) != 0;

    loop {
        {
            let mut manager = PROCESS_MANAGER.lock();

            let target_pid = if target > 0 {
                let pid = ProcessId(target as u64);
                if manager.parent_of(pid) != Some(current) {
                    return Err(());
                }
                Some(pid)
            } else {
                manager.processes.iter().find_map(|(&pid, proc)| {
                    let p = proc.lock();
                    if p.ppid == Some(current) {
                        if let ProcessStatus::Zombie(_) = p.status {
                            Some(pid)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            };

            if let Some((pid, code)) =
                target_pid.and_then(|pid| manager.reap(pid).map(|code| (pid, code)))
            {
                return Ok(Some((pid, code)));
            }

            let has_children = if target > 0 {
                manager.parent_of(ProcessId(target as u64)) == Some(current)
            } else {
                manager
                    .processes
                    .values()
                    .any(|proc| proc.lock().ppid == Some(current))
            };

            if !has_children {
                return Err(());
            }

            if wnohang {
                return Ok(None);
            }
        }

        crate::task::scheduler::sleep(1);
    }
}

pub(crate) fn attach_thread(pid: ProcessId, thread: ThreadId) {
    if let Some(process) = PROCESS_MANAGER.lock().get(pid) {
        process.lock().attach_thread(thread);
    }
}

pub(crate) fn current() -> Option<Arc<InterruptMutex<Process>>> {
    PROCESS_MANAGER.lock().get(identity::current_pid())
}

pub(crate) fn attach_space(pid: ProcessId, space: PhysFrame) {
    if let Some(process) = PROCESS_MANAGER.lock().get(pid) {
        let mut guard = process.lock();
        guard.space = Some(space);
        guard.heap_start = DEFAULT_HEAP_BASE;
        guard.heap_break = DEFAULT_HEAP_BASE;
        guard.mmap_next = DEFAULT_MMAP_BASE;
        guard.sigactions = [crate::task::signal::SigAction::default(); 64];
        guard.pending_signals = 0;
    }
}
pub(crate) fn mmap(pid: ProcessId, addr: u64, len: u64, prot: u64, flags: u64) -> Result<u64, ()> {
    PROCESS_MANAGER.lock().mmap(pid, addr, len, prot, flags)
}
pub(crate) fn mmap_phys(pid: ProcessId, phys_start: u64, len: u64) -> Result<u64, ()> {
    PROCESS_MANAGER.lock().mmap_phys(pid, phys_start, len)
}

pub(crate) fn brk(pid: ProcessId, new_brk: u64) -> Result<u64, ()> {
    PROCESS_MANAGER.lock().update_brk(pid, new_brk)
}

pub(crate) fn take_space(pid: ProcessId) -> Option<mmu::AddressSpace> {
    let process = PROCESS_MANAGER.lock().get(pid)?;

    let root = process.lock().space.take()?;

    Some(mmu::AddressSpace::from_root(root))
}

pub(crate) fn activate_space(pid: ProcessId) {
    let root = PROCESS_MANAGER
        .lock()
        .get(pid)
        .and_then(|process| process.lock().space);

    match root {
        Some(frame) => mmu::activate_root(frame),
        None => mmu::activate_root(mmu::kernel_cr3()),
    }
}

pub(crate) fn with_current_fds<R>(f: impl FnOnce(&mut FdTable) -> R) -> Option<R> {
    let process = current()?;
    let mut guard = process.lock();

    Some(f(&mut guard.fds))
}

pub(crate) fn send_signal(pid: ProcessId, sig: usize) -> bool {
    let manager = PROCESS_MANAGER.lock();
    let Some(proc) = manager.get(pid) else {
        return false;
    };
    if sig == 0 {
        return true;
    }
    if sig <= 64 {
        let mut p = proc.lock();
        p.pending_signals |= 1u64 << (sig - 1);
        for &thread_id in &p.threads {
            crate::task::scheduler::wake(thread_id);
        }
    }
    true
}

pub(crate) fn send_signal_pgrp(pgid: ProcessId, sig: usize) -> bool {
    let manager = PROCESS_MANAGER.lock();
    let mut found = false;
    for proc in manager.processes.values() {
        let mut p = proc.lock();
        if p.pgid == pgid || p.pid == pgid {
            found = true;
            if sig > 0 && sig <= 64 {
                p.pending_signals |= 1u64 << (sig - 1);
                for &thread_id in &p.threads {
                    crate::task::scheduler::wake(thread_id);
                }
            }
        }
    }
    found
}

pub(crate) fn has_pending_signals() -> bool {
    if let Some(proc) = current() {
        let p = proc.lock();
        (p.pending_signals & !p.blocked_signals) != 0
    } else {
        false
    }
}
