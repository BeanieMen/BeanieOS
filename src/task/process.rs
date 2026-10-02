#![allow(dead_code)]

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec::Vec};

use spin::Mutex;

use crate::{
    arch::interrupts::vectors::ticks,
    task::identity::{self, ProcessId, ThreadId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
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
            status: ProcessStatus::Running,
            cwd: String::from(cwd),
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

    fn live_threads(&self) -> usize {
        self.threads.len()
    }
}

pub struct ProcessManager {
    processes: BTreeMap<ProcessId, Arc<Mutex<Process>>>,
    initialized: bool,
}

impl ProcessManager {
    const fn new() -> Self {
        ProcessManager {
            processes: BTreeMap::new(),
            initialized: false,
        }
    }

    pub fn init(&mut self) {
        if self.initialized {
            return;
        }

        let kernel = Arc::new(Mutex::new(Process::boot()));

        self.processes.insert(ProcessId::KERNEL, kernel);
        identity::set_current_pid(ProcessId::KERNEL);
        self.initialized = true;
    }

    pub fn create(&mut self, name: &str, cwd: &str) -> ProcessId {
        if !self.initialized {
            self.init();
        }

        let process = Process::new(name, Some(identity::current_pid()), cwd);
        let pid = process.pid;

        self.processes.insert(pid, Arc::new(Mutex::new(process)));

        pid
    }

    pub fn exit(&mut self, pid: ProcessId, exit_code: i32) {
        let Some(process) = self.processes.get(&pid) else {
            return;
        };

        let mut process = process.lock();

        process.status = ProcessStatus::Zombie(exit_code);
        process.exit_code = Some(exit_code);
    }

    // pub fn exit_current(&mut self, exit_code: i32) {
    //     let current = CURRENT
    //     self.exit(current_pid, exit_code);
    // }

    pub fn reap(&mut self, pid: ProcessId) -> Option<i32> {
        let process = self.processes.get(&pid)?;

        let code = match process.lock().status {
            ProcessStatus::Zombie(code) => code,
            ProcessStatus::Dead => return None,
            ProcessStatus::Running => return None,
        };

        self.processes.remove(&pid);

        Some(code)
    }

    pub fn wait(&mut self, target: ProcessId) -> Option<i32> {
        let current = identity::current_pid();

        loop {
            if let Some(code) = self.reap(target) {
                return Some(code);
            }

            let process = self.processes.get(&target)?;

            if process.lock().ppid != Some(current) {
                return None;
            }

            crate::task::scheduler::sleep(1);
        }
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

    pub fn list(&self) -> Vec<ProcessInfo> {
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

pub static PROCESS_MANAGER: Mutex<ProcessManager> = Mutex::new(ProcessManager::new());

pub fn init() {
    PROCESS_MANAGER.lock().init();
}

pub fn create(name: &str, cwd: &str) -> ProcessId {
    PROCESS_MANAGER.lock().create(name, cwd)
}

pub fn exit(pid: ProcessId, code: i32) {
    PROCESS_MANAGER.lock().exit(pid, code);
}

pub fn reap(pid: ProcessId) -> Option<i32> {
    PROCESS_MANAGER.lock().reap(pid)
}

pub fn wait(target: ProcessId) -> Option<i32> {
    PROCESS_MANAGER.lock().wait(target)
}

pub fn kill(pid: ProcessId) -> Result<(), &'static str> {
    PROCESS_MANAGER.lock().kill(pid)
}

pub fn list() -> Vec<ProcessInfo> {
    PROCESS_MANAGER.lock().list()
}
