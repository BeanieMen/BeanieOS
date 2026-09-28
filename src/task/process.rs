use core::sync::atomic::{AtomicU64, Ordering};

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use spin::Mutex;
use x86_64::structures::paging::PhysFrame;

use crate::{arch::interrupts::vectors::ticks, task::scheduler::ThreadId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessId {
    pub id: u64,
}

impl ProcessId {
    pub const KERNEL: ProcessId = ProcessId { id: 0 };
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        ProcessId {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcessStatus {
    Created,
    Ready,
    Running,
    Sleeping(u64),      // ticks remaining till wakeup
    Waiting(ProcessId), // waiting for a child to exit
    Zomibie(i32),       // exited, but not reaped by parent
    Dead,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: ProcessId,
    pub ppid: Option<ProcessId>,
    pub name: String,
    pub status: ProcessStatus,
    pub cwd: String,
    pub threads_count: usize,
    pub uptime_ticks: u64,
}

/// A Process represents an isolated execution container with its own
/// metadata, page table (CR3), threads, and resource tables.
pub struct Process {
    pub pid: ProcessId,
    pub ppid: Option<ProcessId>,
    pub name: String,
    pub status: ProcessStatus,
    pub cwd: String,
    pub page_table: Option<PhysFrame>,
    pub threads: Vec<ThreadId>,
    pub exit_code: Option<i32>,
    pub created_at: u64,
}

impl Process {
    pub fn new_kernel() -> Self {
        Process {
            pid: ProcessId::KERNEL,
            ppid: None,
            name: String::from("kernel"),
            status: ProcessStatus::Running,
            cwd: String::from("/"),
            page_table: None,
            threads: Vec::new(),
            exit_code: None,
            created_at: ticks(),
        }
    }
    pub fn new(name: &str, ppid: Option<ProcessId>, cwd: &str) -> Self {
        Process {
            pid: ProcessId::new(),
            ppid,
            name: String::from(name),
            status: ProcessStatus::Created,
            cwd: String::from(cwd),
            page_table: None,
            threads: Vec::new(),
            exit_code: None,
            created_at: ticks(),
        }
    }

    pub fn attach_thread(&mut self, thread_id: ThreadId) {
        if (!self.threads.contains(&thread_id)) {
            self.threads.push(thread_id);
        }
    }

    pub fn detach_thread(&mut self, thread_id: ThreadId) {
        if let Some(pos) = self.threads.iter().position(|&id| id == thread_id) {
            self.threads.remove(pos);
        }
    }
}
pub struct ProcessManager {
    pub processes: BTreeMap<ProcessId, Arc<Mutex<Process>>>,
    current_pid: ProcessId,
    initialized: bool,
}
impl ProcessManager {
    const fn new() -> Self {
        ProcessManager {
            processes: BTreeMap::new(),
            current_pid: ProcessId::KERNEL,
            initialized: false,
        }
    }

    pub fn init(&mut self) {
        if self.initialized {
            return;
        }

        let kernel_proc = Arc::new(Mutex::new(Process::new_kernel()));
        self.processes.insert(ProcessId::KERNEL, kernel_proc);
        self.current_pid = ProcessId::KERNEL;
        self.initialized = true;
    }

    pub fn create_process(&mut self, name: &str, cwd: &str) -> ProcessId {
        if !self.initialized {
            self.init();
        }

        let parent = Some(self.current_pid);
        let proc = Process::new(name, parent, cwd);
        let pid = proc.pid;
        self.processes.insert(pid, Arc::new(Mutex::new(proc)));
        pid
    }

    pub fn exit_process(&mut self, pid: ProcessId, exit_code: i32) {
        if let Some(proc_arc) = self.processes.get(&pid) {
            let mut proc = proc_arc.lock();
            proc.status = ProcessStatus::Zomibie(exit_code);
            proc.exit_code = Some(exit_code);
        }

        // wake any parent waiting on this child
        for proc_arc in self.processes.values() {
            let mut p = proc_arc.lock();
            if p.status == ProcessStatus::Waiting(pid) {
                p.status = ProcessStatus::Ready;
            }
        }
    }

    pub fn kill(&mut self, pid: ProcessId) -> Result<(), &'static str> {
        if pid == ProcessId::KERNEL {
            return Err("Cannot kill kernel process");
        }
        if let Some(proc_arc) = self.processes.get(&pid) {
            proc_arc.lock().status = ProcessStatus::Dead;
            self.processes.remove(&pid);
            Ok(())
        } else {
            Err("PID not found")
        }
    }
    pub fn current_pid(&self) -> ProcessId {
        self.current_pid
    }

    pub fn set_current_pid(&mut self, pid: ProcessId) {
        self.current_pid = pid;
    }

    pub fn list_processes(&self) -> Vec<ProcessInfo> {
        let now = ticks();

        self.processes
            .values()
            .map(|arc| {
                let p = arc.lock();

                ProcessInfo {
                    pid: p.pid,
                    ppid: p.ppid,
                    name: p.name.clone(),
                    status: p.status,
                    cwd: p.cwd.clone(),
                    threads_count: p.threads.len(),
                    uptime_ticks: now.saturating_sub(p.created_at),
                }
            })
            .collect()
    }
}

pub static PROCESS_MANAGER: Mutex<ProcessManager> = Mutex::new(ProcessManager::new());

pub fn init() {
    PROCESS_MANAGER.lock().init();
}

pub fn create_process(name: &str, cwd: &str) -> ProcessId {
    PROCESS_MANAGER.lock().create_process(name, cwd)
}

pub fn exit_current(exit_code: i32) {
    let pid = PROCESS_MANAGER.lock().current_pid();
    PROCESS_MANAGER.lock().exit_process(pid, exit_code);
    crate::task::scheduler::exit();
}
pub fn wait_pid(target_pid: ProcessId) -> Result<i32, &'static str> {
    loop {
        {
            let mut pm = PROCESS_MANAGER.lock();
            let current = pm.current_pid;
            let proc_arc = match pm.processes.get(&target_pid) {
                Some(p) => p,
                None => return Err("Process not found"),
            };
            let proc = proc_arc.lock();
            if proc.ppid != Some(current) {
                return Err("Not a child of the current process");
            }
            match proc.status {
                ProcessStatus::Zomibie(code) => {
                    drop(proc);
                    drop(proc_arc);
                    pm.processes.remove(&target_pid);
                    return Ok(code);
                }
                ProcessStatus::Dead => {
                    drop(proc);
                    drop(proc_arc);
                    pm.processes.remove(&target_pid);
                    return Ok(-1);
                }
                _ => {
                    if let Some(current_arc) = pm.processes.get(&current) {
                        current_arc.lock().status = ProcessStatus::Waiting(target_pid);
                    }
                }
            }
        }
        crate::task::scheduler::yield_now();
    }
}

pub fn list() -> Vec<ProcessInfo> {
    PROCESS_MANAGER.lock().list_processes()
}
