use core::sync::atomic::{AtomicU64, Ordering};

use alloc::{
    collections::BTreeMap,
    string::String,
    sync::Arc,
    vec::Vec,
};

use spin::Mutex;
use x86_64::structures::paging::PhysFrame;

use crate::{
    arch::interrupts::vectors::ticks,
    task::{scheduler, thread::ThreadId},
};

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
    Sleeping(u64),
    Waiting(ProcessId),
    Zombie(i32),
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
    fn boot() -> Self {
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

    fn new(name: &str, ppid: Option<ProcessId>, cwd: &str) -> Self {
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

    pub fn attach_thread(&mut self, id: ThreadId) {
        if !self.threads.contains(&id) {
            self.threads.push(id);
        }
    }

    pub fn detach_thread(&mut self, id: ThreadId) {
        if let Some(index) = self.threads.iter().position(|&thread| thread == id) {
            self.threads.remove(index);
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

        let kernel = Arc::new(Mutex::new(Process::boot()));

        self.processes.insert(ProcessId::KERNEL, kernel);
        self.current_pid = ProcessId::KERNEL;
        self.initialized = true;
    }

    pub fn create_process(&mut self, name: &str, cwd: &str) -> ProcessId {
        if !self.initialized {
            self.init();
        }

        let process = Process::new(name, Some(self.current_pid), cwd);
        let pid = process.pid;

        self.processes.insert(pid, Arc::new(Mutex::new(process)));

        pid
    }

    pub fn exit_process(&mut self, pid: ProcessId, exit_code: i32) {
        if let Some(process) = self.processes.get(&pid) {
            let mut process = process.lock();

            process.status = ProcessStatus::Zombie(exit_code);
            process.exit_code = Some(exit_code);
        }

        self.wake_waiting_parents(pid);
    }

    pub fn kill(&mut self, pid: ProcessId) -> Result<(), &'static str> {
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
            .map(|process| {
                let process = process.lock();

                ProcessInfo {
                    pid: process.pid,
                    ppid: process.ppid,
                    name: process.name.clone(),
                    status: process.status,
                    cwd: process.cwd.clone(),
                    threads_count: process.threads.len(),
                    uptime_ticks: now.saturating_sub(process.created_at),
                }
            })
            .collect()
    }

    fn wake_waiting_parents(&mut self, child: ProcessId) {
        for process in self.processes.values() {
            let mut process = process.lock();

            if process.status == ProcessStatus::Waiting(child) {
                process.status = ProcessStatus::Ready;
            }
        }
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

    scheduler::exit()
}

pub fn wait_pid(target: ProcessId) -> Result<i32, &'static str> {
    loop {
        match try_reap(target) {
            Reap::Found(result) => return result,
            Reap::NotChild => return Err("Not a child of the current process"),
            Reap::NotFound => return Err("Process not found"),
            Reap::StillRunning => scheduler::yield_now(),
        }
    }
}

enum Reap {
    Found(Result<i32, &'static str>),
    NotChild,
    NotFound,
    StillRunning,
}

fn try_reap(target: ProcessId) -> Reap {
    let mut manager = PROCESS_MANAGER.lock();
    let current = manager.current_pid;

    let Some(process) = manager.processes.get(&target) else {
        return Reap::NotFound;
    };

    if process.lock().ppid != Some(current) {
        return Reap::NotChild;
    }

    let code = match process.lock().status {
        ProcessStatus::Zombie(code) => code,
        ProcessStatus::Dead => -1,
        _ => {
            if let Some(parent) = manager.processes.get(&current) {
                parent.lock().status = ProcessStatus::Waiting(target);
            }

            return Reap::StillRunning;
        }
    };

    manager.processes.remove(&target);

    Reap::Found(Ok(code))
}

pub fn list() -> Vec<ProcessInfo> {
    PROCESS_MANAGER.lock().list_processes()
}
