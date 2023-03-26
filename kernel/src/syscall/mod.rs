mod flags;
mod fs;
mod process;
mod sync;
mod thread;

use crate::{
    task::{current_process, current_trap_ctx, exit_current_and_run_next},
    utils::error::code,
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
    GETCWD, 17,
    DUP, 24,
    FCNTL64, 25,
    IOCTL, 29,
    UNLINKAT, 35,
    LINKAT, 37,
    OPENAT, 56,
    CLOSE, 57,
    PIPE, 59,
    READ, 63,
    WRITE, 64,
    READV, 65,
    WRITEV, 66,
    PPOLL, 73,
    NEWFSTATAT, 79,
    NEWFSTAT, 80,
    EXIT, 93,
    EXIT_GROUP, 94,
    SET_TID_ADDRESS, 96,
    SLEEP, 101,
    CLOCK_GETTIME, 113,
    YIELD, 124,
    KILL, 129,
    SIGACTION, 134,
    SIGPROCMASK, 135,
    SET_PRIORITY, 140,
    SETPGID, 154,
    GETPGID, 155,
    UNAME, 160,
    GET_TIME, 169,
    GETPID, 172,
    GETPPID, 173,
    GETUID, 174,
    GETEUID, 175,
    GETGID, 176,
    GETEGID, 177,
    GETTID, 178,
    BRK, 214,
    MUNMAP, 215,
    CLONE, 220,
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
        GETCWD => sys_getcwd(args[0] as _, args[1]),
        DUP => sys_dup(args[0]),
        FCNTL64 => sys_fcntl64(args[0], args[1], args[2]),
        IOCTL => sys_ioctl(args[0], args[1], args[2]),
        // UNLINKAT => sys_unlinkat(args[1] as *const u8),
        // LINKAT => sys_linkat(args[1] as *const u8, args[3] as *const u8),
        OPENAT => sys_openat(args[0], args[1] as _, args[2] as _, args[3] as _),
        CLOSE => sys_close(args[0]),
        // PIPE => sys_pipe(args[0] as *mut usize),
        READ => sys_read(args[0], args[1] as _, args[2]),
        WRITE => sys_write(args[0], args[1] as _, args[2]),
        READV => sys_readv(args[0], args[1] as _, args[2]),
        WRITEV => sys_writev(args[0], args[1] as _, args[2]),
        PPOLL => sys_ppoll(),
        NEWFSTATAT => sys_fstatat(args[0], args[1] as _, args[2] as _, args[3]),
        // FSTAT => sys_fstat(args[0], args[1] as *mut Stat),
        EXIT | EXIT_GROUP => sys_exit(args[0] as _),
        SET_TID_ADDRESS => sys_set_tid_address(args[0] as _),
        SLEEP => sys_sleep(args[0]),
        CLOCK_GETTIME => sys_clock_gettime(args[0] as _, args[1] as _),
        YIELD => sys_yield(),
        SIGACTION => sys_sigaction(args[0], args[1] as _, args[2] as _),
        SIGPROCMASK => sys_sigprocmask(args[0], args[1] as _, args[2] as _, args[3]),
        // SET_PRIORITY => sys_set_priority(args[0] as isize),
        SETPGID => sys_setpgid(args[0], args[1]),
        GETPGID => sys_getpgid(args[0]),
        UNAME => sys_uname(args[0] as _),
        GETPID => sys_getpid(),
        GETPPID => sys_getppid(),
        GETUID | GETEUID | GETGID | GETEGID => Ok(0), // TODO: 目前不实现用户和用户组相关的部分
        GETTID => sys_gettid(),
        BRK => sys_brk(args[0]),
        CLONE => sys_clone(args[0], args[1], args[2], args[3], args[4]),
        EXEC => sys_exec(args[0] as _, args[1] as _),
        WAITPID => sys_waitpid(args[0] as _, args[1] as _),
        GET_TIME => sys_get_time_of_day(args[0] as _, args[1]),
        // MUNMAP => sys_munmap(args[0], args[1]),
        MMAP => sys_mmap(
            args[0],
            args[1],
            args[2] as _,
            args[3] as _,
            args[4] as _,
            args[5],
        ),
        SPAWN => sys_spawn(args[0] as _),
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
            log::error!("[kernel] Unsupported id: {}", id);
            exit_current_and_run_next(-10);
        }
    };
    let curr_pid = current_process().pid.0;
    match ret {
        Ok(ret) => {
            // 读入标准输入、写入标准输出、写入标准错误、INITPROC 和 shell 都不关心
            if !((id == READ || id == READV) && args[0] == 0
                || (id == WRITE || id == WRITEV) && (args[0] == 1 || args[0] == 2)
                || id == PPOLL) // TODO: 暂时 PPOLL 也忽略
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
            // 等待进程的 EAGAIN 可以忽视
            if !(id == WAITPID && errno == code::EAGAIN) {
                log::info!(
                    "[kernel] pid: {curr_pid}, syscall: {}, return {errno:?}",
                    name(id)
                );
            }
            errno.as_isize()
        }
    }
}
