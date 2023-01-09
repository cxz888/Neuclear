//! Process management syscalls

use crate::config::MAX_SYSCALL_NUM;
use crate::mm::{PageTable, VirtAddr};
use crate::task::{current_page_table, current_process, current_task};
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

/// 功能：将当前进程的地址空间清空并加载一个特定的可执行文件，返回用户态后开始它的执行。
///
/// 参数：
/// - 字符串 path 给出了要加载的可执行文件的名字；
/// - 字符串数组 args 给出了参数列表。其最后一个元素必须是一个 0
///
/// 返回值：如果出错的话（如找不到名字相符的可执行文件）则返回 -1。
///
/// 注意：path 必须以 "\0" 结尾，否则内核将无法确定其长度
///
/// syscall ID：221
pub fn sys_exec(path: *const u8, mut args: *const usize) -> isize {
    let page_table = current_page_table();

    let ret = || -> Option<isize> {
        let path = page_table.translate_str(path)?;
        let mut args_vec: Vec<String> = Vec::new();
        // 收集参数列表
        loop {
            let &arg_str_ptr = page_table.trans_va_as_ref::<usize>(VirtAddr(args as usize))?;
            if arg_str_ptr == 0 {
                break;
            }
            args_vec.push(page_table.translate_str(arg_str_ptr as *const u8)?);
            unsafe {
                args = args.add(1);
            }
        }
        let process = current_process();
        let argc = args_vec.len();
        process.exec(&path, args_vec);
        Some(argc as isize)
    }();
    ret.unwrap_or(-1)
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
        let page_table = PageTable::from_token(inner.memory_set.token());
        *page_table
            .trans_va_as_mut(VirtAddr(exit_code_ptr as usize))
            .unwrap() = exit_code;
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
    todo!()
}

pub fn sys_set_priority(_prio: isize) -> isize {
    todo!()
}

pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    todo!()
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    todo!()
}

pub fn sys_spawn(_path: *const u8) -> isize {
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

/// 功能：设置线程控制块中 `clear_child_tid` 的值为 `tidptr`
///
/// 参数：
/// - `tidptr`
///
/// 返回值：总是返回调用者线程的 tid。
///
/// 错误：永不错误。
///
/// syscall ID：96
pub fn sys_set_tid_address(tidptr: *const i32) -> isize {
    // NOTE: 在 linux 手册中，`tidptr` 的类型是 int*。这里设置为 i32，是参考 libc crate 设置 c_int=i32
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    inner.clear_child_tid = tidptr as usize;
    inner.res.as_ref().unwrap().tid as isize
}

/// 功能：将 program break 设置为 `brk`。高于当前堆顶会分配空间，低于则会释放空间
///
/// 参数：
/// - `brk`
///
/// 返回值：
///
/// - 如 `brk` 为 0，返回当前堆顶。
/// - 否则，分配成功返回 0，失败返回 -1。
///
/// 错误：永不错误。
///
/// syscall ID：96
pub fn sys_brk(brk: usize) -> isize {
    todo!("impl sys_brk")
}
