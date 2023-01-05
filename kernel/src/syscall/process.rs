//! Process management syscalls

use crate::config::MAX_SYSCALL_NUM;
use crate::fs::OpenFlags;
use crate::mm::{translated_mut, PageTable, VirtAddr};
use crate::task::processor::{
    current_page_table, current_process, current_task, current_user_token,
};
use crate::task::{exit_current_and_run_next, suspend_current_and_run_next, TaskStatus};
use crate::timer::get_time_us;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub fn sys_exit(exit_code: i32) -> ! {
    // debug!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next(exit_code);
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    current_task().unwrap().process.upgrade().unwrap().pid() as isize
}

/// Syscall Fork which returns 0 for child process and child_pid for parent process
pub fn sys_fork() -> isize {
    let current_process = current_process();
    let new_process = current_process.fork();
    let new_pid = new_process.pid();
    // modify trap context of new_task, because it returns immediately after switching
    let new_process_inner = new_process.inner_exclusive_access();
    let task = new_process_inner.tasks[0].as_ref().unwrap();
    let trap_ctx = task.inner_exclusive_access().trap_ctx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_ctx.x[10] = 0;
    new_pid as isize
}

/// Syscall Exec which accepts the elf path
pub fn sys_exec(path: *const u8, mut args: *const usize) -> isize {
    todo!()
    // let page_table = PageTable::from_token(current_user_token());

    // let ret = || -> Option<isize> {
    //     let path = page_table.translate_str(path)?;
    //     let mut args_vec: Vec<String> = Vec::new();
    //     // 收集参数列表
    //     loop {
    //         let &arg_str_ptr = page_table.translate_va_as_ref::<u8>(VirtAddr(args as usize))?;
    //         if arg_str_ptr == 0 {
    //             break;
    //         }
    //         args_vec.push(page_table.translate_str(arg_str_ptr as *const u8)?);
    //         unsafe {
    //             args = args.add(1);
    //         }
    //     }
    //     let app_inode = open_file(&path, OpenFlags::RDONLY)?;
    //     let all_data = app_inode.read_all();
    //     let process = current_process();
    //     let argc = args_vec.len();
    //     process.exec(all_data.as_slice(), args_vec);
    //     Some(argc as isize)
    // }();
    // ret.unwrap_or(-1)
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let process = current_process();
    // find a child process

    // ---- access current TCB exclusively
    let mut inner = process.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.pid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB lock exclusively
        p.inner_exclusive_access().is_zombie && (pid == -1 || pid as usize == p.pid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after removing from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.pid();
        // ++++ temporarily access child TCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_mut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB lock automatically
}

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

const MICRO_PER_SEC: usize = 1_000_000;

pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let page_table = current_page_table();
    if let Some(ts) = page_table.translate_va_to_pa(VirtAddr(ts as usize)) {
        let ts = ts.as_mut::<TimeVal>();
        let us = get_time_us();
        ts.sec = us / MICRO_PER_SEC;
        ts.usec = us % MICRO_PER_SEC;
        0
    } else {
        -1
    }
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: TaskStatus,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    pub time: usize,
}

pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    -1
}

pub fn sys_set_priority(_prio: isize) -> isize {
    -1
}

pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    -1
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    -1
}

pub fn sys_spawn(path: *const u8) -> isize {
    todo!()
    // let page_table = current_page_table();
    // let path = if let Some(path) = page_table.translate_str(path) {
    //     path
    // } else {
    //     return -1;
    // };
    // if let Some(app_inode) = open_file(&path, OpenFlags::RDONLY) {
    //     let current_process = current_process();
    //     current_process.spawn(&app_inode.read_all())
    // } else {
    //     -1
    // }
}
