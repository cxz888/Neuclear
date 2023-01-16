use crate::{
    error::{code, Result},
    mm::kernel_token,
    task::{add_task, current_task, TaskControlBlock},
    trap::{trap_handler, TrapContext},
};
use alloc::sync::Arc;

pub fn sys_thread_create(entry: usize, arg: usize) -> Result {
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    // create a new thread
    let new_task = Arc::new(TaskControlBlock::new(&process, true));
    let mut new_task_inner = new_task.inner_exclusive_access();
    let new_task_ustack_top = new_task_inner.res.as_ref().unwrap().user_stack_high_addr();
    let new_task_tid = new_task_inner.res.as_ref().unwrap().tid;
    let new_task_trap_ctx = new_task_inner.trap_ctx();
    *new_task_trap_ctx = TrapContext::app_init_context(
        entry,
        new_task_ustack_top,
        kernel_token(),
        new_task.kernel_stack.top(),
        trap_handler as usize,
    );
    new_task_trap_ctx.x[10] = arg;

    let mut inner = process.inner_exclusive_access();
    // add new thread to current process
    let tasks = &mut inner.tasks;
    while tasks.len() < new_task_tid + 1 {
        tasks.push(None);
    }
    tasks[new_task_tid] = Some(Arc::clone(&new_task));
    // add new task to scheduler
    add_task(Arc::clone(&new_task));
    Ok(new_task_tid as isize)
}

pub fn sys_gettid() -> Result {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    Ok(tid as isize)
}

/// thread does not exist, return -1
/// thread has not exited yet, return -2
/// otherwise, return thread's exit code
pub fn sys_waittid(tid: usize) -> Result {
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    let task_inner = task.inner_exclusive_access();
    let mut inner = process.inner_exclusive_access();
    // a thread cannot wait for itself
    if task_inner.res.as_ref().unwrap().tid == tid {
        return Err(code::TEMP);
    }
    let waited_task = inner.tasks[tid].as_ref();
    let waited_task = waited_task.ok_or(code::TEMP)?;
    let exit_code = waited_task
        .inner_exclusive_access()
        .exit_code
        .ok_or(code::TEMP)?;
    // dealloc the exited thread
    inner.tasks[tid] = None;
    Ok(exit_code as isize)
}
