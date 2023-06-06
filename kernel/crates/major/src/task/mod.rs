//! 进程/线程管理
//!
//! 请注意，[`__switch`] 函数会发生控制流的突变，使用时务必小心。具体而言就是，突变之前的 Arc、inner() 借用需要全部释放

mod clone_flags;
mod context;
mod loader;
mod manager;
mod process;
mod processor;
mod resource;
mod switch;
mod thread;
mod time;

pub use clone_flags::CloneFlags;
pub use manager::add_task;
pub use process::{check_cstr, check_ptr, check_ptr_mut, check_slice, check_slice_mut};
pub use processor::{curr_page_table, curr_process, curr_task, curr_trap_ctx, run_tasks};
pub use thread::{ThreadControlBlock, ThreadStatus};
pub use time::{add_timer, check_timer};

use alloc::{string::ToString, sync::Arc, vec, vec::Vec};
use context::TaskContext;
use core::mem;
use lazy_static::*;
use manager::fetch_task;
use process::{ProcessControlBlock, ProcessControlBlockInner};
use processor::{schedule, take_curr_task};
use resource::{pid_alloc, KernelStack, PidHandle, ThreadUserRes};
use switch::__switch;

#[no_mangle]
pub extern "C" fn __block_curr_and_run_next() {
    let task_ctx_ptr;
    {
        let task = take_curr_task().unwrap();
        let mut task_inner = task.inner();
        task_ctx_ptr = &mut task_inner.task_ctx as *mut TaskContext;
        task_inner.thread_status = ThreadStatus::Blocking;
    }
    schedule(task_ctx_ptr);
}

/// 挂起当前任务，并切换到下一个任务
#[no_mangle]
pub extern "C" fn __suspend_curr_and_run_next() {
    let task_ctx_ptr;
    let task = take_curr_task().unwrap();
    {
        let mut task_inner = task.inner();
        task_ctx_ptr = &mut task_inner.task_ctx as *mut TaskContext;
        task_inner.thread_status = ThreadStatus::Ready;
        drop(task_inner);
    }
    add_task(task);

    schedule(task_ctx_ptr);
}

/// 退出当前线程，并切换到下一个任务，如果是主线程则回收进程资源
#[no_mangle]
pub extern "C" fn __exit_curr_and_run_next(exit_code: i32) -> ! {
    let task = take_curr_task().unwrap();
    let mut task_inner = task.inner();
    let process = task.process.upgrade().unwrap();
    let tid = task_inner.res.as_ref().unwrap().tid;
    task_inner.exit_code = Some(exit_code);
    task_inner.res = None;

    drop(task_inner);
    drop(task);

    // 此处暂不释放线程，因为当前仍在使用它的内核栈
    // 当 `sys_waittid` 被调用时，它的资源才会被回收

    // 主线程退出，则回收所有进程资源
    if tid == 0 {
        log::info!("Process {} exits", process.pid.0);
        let mut process_inner = process.inner();
        process_inner.is_zombie = true;
        process_inner.exit_code = exit_code;

        // 将该进程的所有子进程交给 INITPROC 来回收
        if process.pid.0 != 0 {
            let mut initproc_inner = INITPROC.inner();
            for child in mem::take(&mut process_inner.children) {
                child.inner().parent = Arc::downgrade(&INITPROC);
                initproc_inner.children.push(child);
            }
        }

        // 释放用户资源，即 tid 和用户栈。需要在清理整个 `memory_set` 之前清理，否则会释放两次
        // 虽然先收集再 clear() 很奇怪，但 TaskUserRes 的 drop 需要借用 process_inner
        // 所以需要先将这里的 process_inner drop 后才可以清除
        let mut recycle_res = Vec::<ThreadUserRes>::new();
        for task in process_inner.threads.iter().filter(|t| t.is_some()) {
            let task = task.as_ref().unwrap();
            let mut task_inner = task.inner();
            if let Some(res) = task_inner.res.take() {
                recycle_res.push(res);
            }
        }
        drop(process_inner);
        recycle_res.clear();

        let mut process_inner = process.inner();
        process_inner.children.clear();
        process_inner.memory_set.recycle_all_pages();
        process_inner.fd_table.clear();
    }

    drop(process);

    // 退出了，所以无需保存 task context
    let mut unused = TaskContext::zero_init();
    schedule(&mut unused as _);
    unreachable!()
}

// FIXME: 权宜之计罢了
#[cfg(feature = "test")]
static EXEC_TEST_ELF: &[u8] = include_bytes!("../exec_test.elf");

lazy_static! {
    pub static ref INITPROC: Arc<ProcessControlBlock> = {
        #[cfg(feature = "test")]
        return ProcessControlBlock::from_elf(
            "exec_test".to_string(),
            vec!["exec_test".to_string()],
            EXEC_TEST_ELF,
        )
        .expect("INITPROC Failed");
        #[cfg(not(feature = "test"))]
        ProcessControlBlock::from_path("initproc".to_string(), vec!["initproc".to_string()])
            .expect("INITPROC Failed.")
    };
}

/// List all files in the filesystems
pub fn list_apps() {
    println!("/**** APPS ****");
    use vfs::{Entry, Fs};
    for app in filesystem::VIRTUAL_FS.root_dir().ls().unwrap() {
        println!("{}", app);
    }
    println!("**************/");
}
