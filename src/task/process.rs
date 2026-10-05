use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};

use x86_64::structures::paging::PhysFrame;

use crate::{
    arch::interrupts::vectors::ticks,
    arch::lock::InterruptMutex,
    memory::mmu,
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
}

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

    pub(crate) fn get(&self, pid: ProcessId) -> Option<Arc<InterruptMutex<Process>>> {
        self.processes.get(&pid).cloned()
    }

    // Parent of `pid`, so a waiter can tell "still running" from "not mine".
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
        // The manager lock is dropped before every sleep. `Scheduler::spawn`
        // and `Scheduler::remove` both reach back into PROCESS_MANAGER, so the
        // scheduler's lock is always the outer one; sleeping with it held would
        // invert that and deadlock against any thread being created.
        {
            let mut manager = PROCESS_MANAGER.lock();

            if let Some(code) = manager.reap(target) {
                return Some(code);
            }

            // Not a child of ours: waiting would never end, so say so now.
            if manager.parent_of(target) != Some(current) {
                return None;
            }
        }

        crate::task::scheduler::sleep(1);
    }
}

// Records `thread` as belonging to `pid`. Paired with `detach_thread` by the
// scheduler, or a reaped thread stays counted.
pub(crate) fn attach_thread(pid: ProcessId, thread: ThreadId) {
    if let Some(process) = PROCESS_MANAGER.lock().get(pid) {
        process.lock().attach_thread(thread);
    }
}

// Manager lock, then process lock: the same order as everything here.
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
        process.lock().space = Some(space);
    }
}

fn activate_space(pid: ProcessId) {
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
