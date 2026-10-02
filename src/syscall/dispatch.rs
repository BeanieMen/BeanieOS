use crate::task::{identity::current_pid, process::PROCESS_MANAGER};

use super::numbers::*;

pub fn dispatch(number: u64, args: [u64; 6]) -> u64 {
    match Syscall::try_from(number) {
        _ => {0} // todo
    }
}
// fn sys_exit(code: u64) -> ! {
//     crate::task::process::exit(current_pid(), code as i32);
//     pro
//     loop {x86_64::instructions::hlt();}
// }

fn sys_getpid() -> u64 {
    current_pid().as_u64()
}
