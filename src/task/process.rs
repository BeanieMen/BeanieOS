use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};

use x86_64::structures::paging::PhysFrame;

use crate::{
    arch::{interrupts::vectors::ticks, lock::InterruptMutex},
    memory::{
        allocator::alloc_frame,
        mmu::{self, Mapper},
    },
    paging::phys_to_direct,
    storage::vfs::fd::FdTable,
    task::identity::{self, ProcessId, ThreadId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessStatus {
    Running,
    Zombie(i32),
    Dead,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessInfo {
    pub pid: ProcessId,
    pub ppid: Option<ProcessId>,
    pub name: String,
    pub status: ProcessStatus,
    pub cwd: String,
    pub threads_count: usize,
    pub uptime_ticks: u64,
}

pub(crate) struct Process {
    pub pid: ProcessId,
    pub ppid: Option<ProcessId>,
    pub name: String,
    pub status: ProcessStatus,
    pub cwd: String,
    pub threads: Vec<ThreadId>,
    pub exit_code: Option<i32>,
    pub created_at: u64,
    pub fds: FdTable,
    pub space: Option<PhysFrame>,
    pub heap_start: u64,
    pub heap_break: u64,
}

const DEFAULT_HEAP_BASE: u64 = 0x1000_0000; // 256 MiB mark

impl Process {
    fn boot() -> Self {
        Process {
            pid: ProcessId::KERNEL,
            ppid: None,
            name: String::from("kernel"),
            status: ProcessStatus::Running,
            cwd: String::from("/"),
            threads: Vec::new(),
            exit_code: None,
            created_at: ticks(),
            fds: FdTable::new(),
            space: None,
            heap_start: DEFAULT_HEAP_BASE,
            heap_break: DEFAULT_HEAP_BASE,
        }
    }

    fn new(name: &str, ppid: Option<ProcessId>, cwd: &str) -> Self {
        Process {
            pid: ProcessId::new(),
            ppid,
            name: String::from(name),
            status: ProcessStatus::Running,
            cwd: String::from(cwd),
            threads: Vec::new(),
            exit_code: None,
            created_at: ticks(),
            fds: FdTable::new(),
            space: None,
            heap_start: DEFAULT_HEAP_BASE,
            heap_break: DEFAULT_HEAP_BASE,
        }
    }

    pub(crate) fn attach_thread(&mut self, id: ThreadId) {
        if !self.threads.contains(&id) {
            self.threads.push(id);
        }
    }

    pub(crate) fn detach_thread(&mut self, id: ThreadId) {
        if let Some(index) = self.threads.iter().position(|&thread| thread == id) {
            self.threads.remove(index);
        }
    }

    fn live_threads(&self) -> usize {
        self.threads.len()
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
        let (parent_name, parent_cwd, parent_fds, parent_heap_start, parent_heap_break) = {
            let p = parent.lock();
            (
                p.name.clone(),
                p.cwd.clone(),
                p.fds.clone(),
                p.heap_start,
                p.heap_break,
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

        let child_pid = child.pid;
        self.processes
            .insert(child_pid, Arc::new(InterruptMutex::new(child)));
        Some(child_pid)
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

        if new_page > old_page {
            let mut curr = old_page;
            while curr < new_page {
                let frame = alloc_frame().ok_or(())?;
                let direct = phys_to_direct(frame.start_address().as_u64()) as *mut u8;

                unsafe {
                    core::ptr::write_bytes(direct, 0, 4096);
                }

                space.map(curr, frame, mmu::USER_RW).map_err(|_| ())?;

                curr += 4096;
            }
        }
        core::mem::forget(space); // keep the space alive, as the process still owns it
        p.heap_break = new_brk;
        return Ok(p.heap_break);
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
    }

    pub(crate) fn reap(&mut self, pid: ProcessId) -> Option<i32> {
        let process = self.processes.get(&pid)?;

        let code = match process.lock().status {
            ProcessStatus::Zombie(code) => code,
            ProcessStatus::Dead => return None,
            ProcessStatus::Running => return None,
        };

        self.processes.remove(&pid);

        Some(code)
    }

    fn kill(&mut self, pid: ProcessId) -> Result<(), &'static str> {
        if pid == ProcessId::KERNEL {
            return Err("Cannot kill kernel process");
        }

        match self.processes.get(&pid) {
            Some(process) => {
                process.lock().status = ProcessStatus::Dead;
                self.processes.remove(&pid);
                Ok(())
            }
            None => Err("PID not found"),
        }
    }

    pub(crate) fn list(&self) -> Vec<ProcessInfo> {
        let now = ticks();

        self.processes
            .values()
            .map(|process| {
                let process = process.lock();

                ProcessInfo {
                    pid: process.pid,
                    ppid: process.ppid,
                    name: process.name.clone(),
                    status: process.status,
                    cwd: process.cwd.clone(),
                    threads_count: process.live_threads(),
                    uptime_ticks: now.saturating_sub(process.created_at),
                }
            })
            .collect()
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

pub(crate) fn reap(pid: ProcessId) -> Option<i32> {
    PROCESS_MANAGER.lock().reap(pid)
}

pub(crate) fn wait(target: ProcessId) -> Option<i32> {
    let current = identity::current_pid();

    loop {
        // The manager lock is dropped before every sleep. `Scheduler::spawn` and
        // `Scheduler::remove` both reach back into PROCESS_MANAGER, so the
        // scheduler's lock is always outer; holding this one across the sleep
        // would invert that and deadlock against any thread being created.
        {
            let mut manager = PROCESS_MANAGER.lock();

            if let Some(code) = manager.reap(target) {
                return Some(code);
            }

            if manager.parent_of(target) != Some(current) {
                return None;
            }
        }

        crate::task::scheduler::sleep(1);
    }
}

pub(crate) fn create_child(parent_pid: ProcessId) -> Option<ProcessId> {
    PROCESS_MANAGER.lock().create_child(parent_pid)
}

pub(crate) fn space_root_of(pid: ProcessId) -> Option<PhysFrame> {
    PROCESS_MANAGER.lock().get(pid)?.lock().space
}

pub(crate) fn wait4(target: i64) -> Option<(ProcessId, i32)> {
    let current = identity::current_pid();

    loop {
        {
            let mut manager = PROCESS_MANAGER.lock();

            let target_pid = if target > 0 {
                let pid = ProcessId(target as u64);
                if manager.parent_of(pid) != Some(current) {
                    return None;
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

            if let Some(pid) = target_pid {
                if let Some(code) = manager.reap(pid) {
                    return Some((pid, code));
                }
            }

            let has_children = manager
                .processes
                .values()
                .any(|proc| proc.lock().ppid == Some(current));

            if !has_children {
                return None;
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

pub(crate) fn detach_thread(pid: ProcessId, thread: ThreadId) {
    if let Some(process) = PROCESS_MANAGER.lock().get(pid) {
        process.lock().detach_thread(thread);
    }
}

fn kill(pid: ProcessId) -> Result<(), &'static str> {
    PROCESS_MANAGER.lock().kill(pid)
}

pub(crate) fn list() -> Vec<ProcessInfo> {
    PROCESS_MANAGER.lock().list()
}

pub(crate) fn current() -> Option<Arc<InterruptMutex<Process>>> {
    PROCESS_MANAGER.lock().get(identity::current_pid())
}

fn current_space() -> Option<PhysFrame> {
    let process = current()?;
    let guard = process.lock();

    guard.space
}

pub(crate) fn attach_space(pid: ProcessId, space: PhysFrame) {
    if let Some(process) = PROCESS_MANAGER.lock().get(pid) {
        let mut guard = process.lock();
        guard.space = Some(space);
        guard.heap_start = DEFAULT_HEAP_BASE;
        guard.heap_break = DEFAULT_HEAP_BASE;
    }
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

fn current_cr3() -> usize {
    let process = PROCESS_MANAGER.lock().get(identity::current_pid());
    let space = process.and_then(|process| process.lock().space);

    match space {
        Some(frame) => frame.start_address().as_u64() as usize,
        None => mmu::kernel_cr3().start_address().as_u64() as usize,
    }
}

pub(crate) fn with_current_fds<R>(f: impl FnOnce(&mut FdTable) -> R) -> Option<R> {
    let process = current()?;
    let mut guard = process.lock();

    Some(f(&mut guard.fds))
}
