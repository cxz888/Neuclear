mod flags;
mod fs;
mod process;
mod sync;
mod thread;

use crate::{
    error::code,
    task::{current_process, current_trap_ctx},
};
use fs::*;
use process::*;
use sync::*;
use thread::*;

pub use flags::{MmapFlags, MmapProt};

macro_rules! declare_syscall_id {
    ($($name:tt, $id:literal),*) => {
        $(const $name: usize = $id;)*
        fn name(id: usize) -> &'static str {
            match id {
                $($name => stringify!($name),)*
                _ => unreachable!(),
            }
        }
    };
}

#[rustfmt::skip]
declare_syscall_id!(
    DUP, 24, 
    IOCTL, 29,
    UNLINKAT, 35,
    LINKAT, 37,
    OPEN, 56,
    CLOSE, 57,
    PIPE, 59,
    READ, 63,
    WRITE, 64,
    FSTAT, 80,
    EXIT, 93,
    EXIT_GROUP, 94,
    SET_TID_ADDRESS, 96,
    SLEEP, 101,
    YIELD, 124,
    SET_PRIORITY, 140,
    GET_TIME, 169,
    GETPID, 172,
    GETUID, 174,
    GETEUID, 175,
    GETGID, 176,
    GETEGID, 177,
    GETTID, 178,
    BRK, 214,
    MUNMAP, 215,
    FORK, 220,
    EXEC, 221,
    MMAP, 222,
    WAITPID, 260,
    SPAWN, 400,
    TASK_INFO, 410,
    THREAD_CREATE, 460,
    WAITTID, 462,
    MUTEX_CREATE, 463,
    MUTEX_LOCK, 464,
    MUTEX_UNLOCK, 466,
    SEMAPHORE_CREATE, 467,
    SEMAPHORE_UP, 468,
    SEMAPHORE_DOWN, 470,
    CONDVAR_CREATE, 471,
    CONDVAR_SIGNAL, 472,
    CONDVAR_WAIT, 473
);

/// handle syscall exception with `id` and other arguments
pub fn syscall(id: usize, args: [usize; 6]) -> isize {
    let ret = match id {
        DUP => sys_dup(args[0]),
        IOCTL => sys_ioctl(args[0], args[1], args[2] as _),
        // UNLINKAT => sys_unlinkat(args[1] as *const u8),
        // LINKAT => sys_linkat(args[1] as *const u8, args[3] as *const u8),
        // OPEN => sys_open(args[1] as *const u8, args[2] as u32),
        // CLOSE => sys_close(args[0]),
        // PIPE => sys_pipe(args[0] as *mut usize),
        READ => sys_read(args[0], args[1] as *const u8, args[2]),
        WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        // FSTAT => sys_fstat(args[0], args[1] as *mut Stat),
        EXIT | EXIT_GROUP => sys_exit(args[0] as i32),
        SET_TID_ADDRESS => sys_set_tid_address(args[0] as _),
        SLEEP => sys_sleep(args[0]),
        YIELD => sys_yield(),
        // SET_PRIORITY => sys_set_priority(args[0] as isize),
        GETPID => sys_getpid(),
        GETUID | GETEUID | GETGID | GETEGID => Ok(0), // 目前不实现用户和用户组相关的部分
        GETTID => sys_gettid(),
        BRK => sys_brk(args[0]),
        FORK => sys_fork(),
        EXEC => sys_exec(args[0] as *const u8, args[1] as *const usize),
        WAITPID => sys_waitpid(args[0] as isize, args[1] as *mut i32),
        GET_TIME => sys_get_time(args[0] as *mut TimeVal, args[1]),
        // MUNMAP => sys_munmap(args[0], args[1]),
        MMAP => sys_mmap(
            args[0],
            args[1],
            args[2] as u32,
            args[3] as u32,
            args[4] as i32,
            args[5],
        ),
        SPAWN => sys_spawn(args[0] as *const u8),
        // TASK_INFO => sys_task_info(args[0] as *mut TaskInfo),
        THREAD_CREATE => sys_thread_create(args[0], args[1]),
        WAITTID => sys_waittid(args[0]),
        MUTEX_CREATE => sys_mutex_create(args[0] == 1),
        MUTEX_LOCK => sys_mutex_lock(args[0]),
        MUTEX_UNLOCK => sys_mutex_unlock(args[0]),
        SEMAPHORE_CREATE => sys_semaphore_create(args[0]),
        SEMAPHORE_UP => sys_semaphore_up(args[0]),
        SEMAPHORE_DOWN => sys_semaphore_down(args[0]),
        CONDVAR_CREATE => sys_condvar_create(args[0]),
        CONDVAR_SIGNAL => sys_condvar_signal(args[0]),
        CONDVAR_WAIT => sys_condvar_wait(args[0], args[1]),
        _ => {
            log::error!(
                "[kernel] Unsupport inst pc = {:#x}",
                current_trap_ctx().sepc,
            );
            panic!("[kernel] Unsupported id: {}", id);
        }
    };
    let curr_pid = current_process().pid.0;
    match ret {
        Ok(ret) => {
            // 读入标准输入、写入标准输出、写入标准错误、INITPROC 和 shell 都不关心
            if !(id == READ && args[0] == 0)
                && !(id == WRITE && args[0] == 1)
                && !(id == WRITE && args[0] == 2)
                && curr_pid != 0
                && curr_pid != 1
            {
                log::info!(
                    "[kernel] pid: {curr_pid}, syscall: {}, return {ret} = {ret:#x}",
                    name(id)
                );
            }
            ret
        }
        Err(errno) => {
            if !(id == 260 && errno == code::EAGAIN) {
                log::info!(
                    "[kernel] pid: {curr_pid}, syscall: {}, return {errno:?}",
                    name(id)
                );
            }
            errno.as_isize()
        }
    }
}
