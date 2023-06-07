mod flags;
mod fs;
mod process;
mod sync;
mod thread;

pub use flags::{MmapFlags, MmapProt};

use crate::task::{__exit_curr_and_run_next, curr_process, curr_trap_ctx};
use fs::*;
use process::*;
use sync::*;
use thread::*;
use utils::error::code;

macro_rules! declare_syscall_id {
    ($($name:tt, $id:literal),*) => {
        $(const $name: usize = $id;)*
        fn name(id: usize) -> &'static str {
            match id {
                $($name => stringify!($name),)*
                _ => unreachable!("{}", id),
            }
        }
    };
}

#[rustfmt::skip]
declare_syscall_id!(
    GETCWD, 17,
    DUP, 23,
    DUP3, 24,
    FCNTL64, 25,
    IOCTL, 29,
    MKDIRAT, 34,
    UNLINKAT, 35,
    LINKAT, 37,
    UMOUNT, 39,
    MOUNT, 40,
    CHDIR, 49,
    OPENAT, 56,
    CLOSE, 57,
    PIPE2, 59,
    GETDENTS64, 61,
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
    SCHED_YIELD, 124,
    KILL, 129,
    SIGACTION, 134,
    SIGPROCMASK, 135,
    SETPRIORITY, 140,
    TIMES, 153,
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
    EXECVE, 221,
    MMAP, 222,
    WAIT4, 260,
    SPAWN, 400,
    WAITTID, 462
);

/// handle syscall exception with `id` and other arguments
pub fn syscall(id: usize, args: [usize; 6]) -> isize {
    let curr_pid = curr_process().pid.0;
    // 读入标准输入、写入标准输出、写入标准错误、INITPROC 和 shell 都不关心
    #[cfg(not(feature = "test"))]
    if !((id == READ || id == READV) && args[0] == 0
        || (id == WRITE || id == WRITEV) && (args[0] == 1 || args[0] == 2)
        || id == PPOLL)
        && curr_pid != 0
        && curr_pid != 1
    {
        log::debug!("syscall {} with args {:x?}", name(id), args);
    }
    let ret = match id {
        GETCWD => sys_getcwd(args[0] as _, args[1]),
        DUP => sys_dup(args[0]),
        DUP3 => sys_dup3(args[0], args[1]),
        FCNTL64 => sys_fcntl64(args[0], args[1], args[2]),
        IOCTL => sys_ioctl(args[0], args[1], args[2]),
        MKDIRAT => sys_mkdirat(args[0], args[1] as _, args[2]),
        UNLINKAT => sys_unlinkat(args[0], args[1] as _, args[2] as _),
        LINKAT => sys_linkat(args[1] as _, args[3] as _),
        UMOUNT => sys_umount(args[0] as _, args[1] as _),
        MOUNT => sys_mount(
            args[0] as _,
            args[1] as _,
            args[2] as _,
            args[3],
            args[4] as _,
        ),
        CHDIR => sys_chdir(args[0] as _),
        OPENAT => sys_openat(args[0], args[1] as _, args[2] as _, args[3] as _),
        CLOSE => sys_close(args[0]),
        PIPE2 => sys_pipe2(args[0] as _),
        GETDENTS64 => sys_getdents64(args[0], args[1] as _, args[2]),
        READ => sys_read(args[0], args[1] as _, args[2]),
        WRITE => sys_write(args[0], args[1] as _, args[2]),
        READV => sys_readv(args[0], args[1] as _, args[2]),
        WRITEV => sys_writev(args[0], args[1] as _, args[2]),
        PPOLL => sys_ppoll(),
        NEWFSTATAT => sys_fstatat(args[0], args[1] as _, args[2] as _, args[3]),
        NEWFSTAT => sys_fstat(args[0], args[1] as _),
        EXIT | EXIT_GROUP => sys_exit(args[0] as _),
        SET_TID_ADDRESS => sys_set_tid_address(args[0] as _),
        SLEEP => sys_sleep(args[0] as _),
        CLOCK_GETTIME => sys_clock_gettime(args[0] as _, args[1] as _),
        SCHED_YIELD => sys_yield(),
        SIGACTION => sys_sigaction(args[0], args[1] as _, args[2] as _),
        SIGPROCMASK => sys_sigprocmask(args[0], args[1] as _, args[2] as _, args[3]),
        SETPRIORITY => sys_setpriority(args[0] as _),
        TIMES => sys_times(args[0] as _),
        SETPGID => sys_setpgid(args[0], args[1]),
        GETPGID => sys_getpgid(args[0]),
        UNAME => sys_uname(args[0] as _),
        GETPID => sys_getpid(),
        GETPPID => sys_getppid(),
        GETUID | GETEUID | GETGID | GETEGID => Ok(0), // TODO: 目前不实现用户和用户组相关的部分
        GETTID => sys_gettid(),
        BRK => sys_brk(args[0]),
        MUNMAP => sys_munmap(args[0], args[1]),
        CLONE => sys_clone(args[0], args[1], args[2], args[3], args[4]),
        EXECVE => sys_execve(args[0] as _, args[1] as _, args[2] as _),
        WAIT4 => sys_wait4(args[0] as _, args[1] as _, args[2], args[3]),
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
        WAITTID => sys_waittid(args[0]),
        _ => {
            log::error!("[kernel] Unsupport inst pc = {:#x}", unsafe {
                curr_trap_ctx().sepc
            },);
            log::error!("[kernel] Unsupported id: {}", id);
            __exit_curr_and_run_next(-10);
        }
    };
    match ret {
        Ok(ret) => {
            // 读入标准输入、写入标准输出、写入标准错误、INITPROC 和 shell 都不关心
            if !((id == READ || id == READV) && args[0] == 0
                || (id == WRITE || id == WRITEV) && (args[0] == 1 || args[0] == 2)
                || id == PPOLL)
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
            if !(id == WAIT4 && errno == code::EAGAIN) {
                log::info!(
                    "[kernel] pid: {curr_pid}, syscall: {}, return {errno:?}",
                    name(id)
                );
            }
            errno.as_isize()
        }
    }
}
