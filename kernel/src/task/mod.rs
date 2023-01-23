//! Implementation of process management mechanism
//!
//! Here is the entry for process scheduling required by other modules
//! (such as syscall or clock interrupt).
//! By suspending or exiting the current process, you can
//! modify the process state, manage the process queue through TASK_MANAGER,
//! and switch the control flow through PROCESSOR.
//!
//! Be careful when you see [`__switch`]. Control flow around this function
//! might not be what you expect.

mod clone_flags;
mod context;
mod id;
mod manager;
mod process;
mod processor;
mod switch;
mod thread;

pub use clone_flags::CloneFlags;
pub use context::TaskContext;
pub use id::{kstack_alloc, pid_alloc, KernelStack, PidHandle, ThreadUserRes};
pub use manager::add_task;
pub use process::{ProcessControlBlock, ProcessControlBlockInner};
pub use processor::{
    current_page_table, current_process, current_task, current_trap_ctx, current_trap_ctx_user_va,
    current_user_token, run_tasks,
};
pub use thread::{ThreadControlBlock, ThreadStatus};

use crate::loader::Loader;
use alloc::{sync::Arc, vec::Vec};
use core::mem;
use lazy_static::*;
use manager::fetch_task;
use processor::{schedule, take_current_task};
use switch::__switch;

pub fn block_current_and_run_next() {
    let task = take_current_task().unwrap();
    let mut task_inner = task.inner();
    let task_ctx_ptr = &mut task_inner.task_ctx as *mut TaskContext;
    task_inner.thread_status = ThreadStatus::Blocking;
    drop(task_inner);
    schedule(task_ctx_ptr);
}

/// Make current task suspended and switch to the next task
pub fn suspend_current_and_run_next() {
    // There must be an application running.
    let task = take_current_task().unwrap();

    // ---- access current TCB exclusively
    let mut task_inner = task.inner();

    let task_ctx_ptr = &mut task_inner.task_ctx as *mut TaskContext;
    // Change status to Ready
    task_inner.thread_status = ThreadStatus::Ready;
    drop(task_inner);
    // ---- release current PCB

    // push back to ready queue.
    add_task(task);
    // jump to scheduling cycle
    schedule(task_ctx_ptr);
}

/// Exit current task, recycle process resources and switch to the next task
pub fn exit_current_and_run_next(exit_code: i32) -> ! {
    let task = take_current_task().unwrap();
    let mut task_inner = task.inner();
    let process = task.process.upgrade().unwrap();
    let tid = task_inner.res.as_ref().unwrap().tid;
    // Record exit code
    task_inner.exit_code = Some(exit_code);
    task_inner.res = None;

    // here we do not remove the thread since we are still using the kstack
    // it will be deallocated when sys_waittid is called
    drop(task_inner);
    drop(task);
    // debug!("task {} dropped", tid);

    if tid == 0 {
        let mut process_inner = process.inner();
        // mark this process as a zombie process
        process_inner.is_zombie = true;
        // record exit code of main process
        process_inner.exit_code = exit_code;

        // move all its children to INITPROC
        if process.pid.0 != 0 {
            let mut initproc_inner = INITPROC.inner();
            for child in mem::take(&mut process_inner.children) {
                child.inner().parent = Arc::downgrade(&INITPROC);
                initproc_inner.children.push(child);
            }
        }
        // 虽然先收集再 clear() 很奇怪，但 TaskUserRes 的 drop 需要借用 process_inner
        // 所以需要先将这里的 process_inner drop 后才可以清除
        let mut recycle_res = Vec::<ThreadUserRes>::new();

        // deallocate user res (including tid/trap_ctx/ustack) of all threads
        // it has to be done before we dealloc the whole memory_set
        // otherwise they will be deallocated twice
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
        // debug!("deallocate pcb res");
        process_inner.children.clear();
        // deallocate other data in user space i.e. program code/data section
        process_inner.memory_set.recycle_data_pages();
        // drop file descriptors
        process_inner.fd_table.clear();
    }

    drop(process);

    // we do not have to save task context
    let mut _unused = TaskContext::zero_init();
    schedule(&mut _unused as *mut _);
    unreachable!()
}

lazy_static! {
    /// Creation of initial process
    ///
    /// the name "initproc" may be changed to any other app name like "usertests",
    /// but we have user_shell, so we don't need to change it.
    pub static ref INITPROC: Arc<ProcessControlBlock> = {
        let pcb = ProcessControlBlock::new();
        Loader::load(&mut pcb.inner(), "initproc", Vec::new()).expect("INITPROC Failed.");
        pcb
    };
}

pub fn add_initproc() {
    // INITPROC must be referenced at least once so that it can be initialized
    // through lazy_static
    INITPROC.pid();
}
