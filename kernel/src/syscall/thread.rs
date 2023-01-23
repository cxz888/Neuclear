use crate::{
    memory::kernel_token,
    task::{add_task, current_task, ThreadControlBlock},
    trap::{trap_handler, TrapContext},
    utils::error::{code, Result},
};
use alloc::sync::Arc;

pub fn sys_thread_create(entry: usize, arg: usize) -> Result {
    let thread = current_task().unwrap();
    let process = thread.process.upgrade().unwrap();
    // create a new thread
    let new_thread = Arc::new(ThreadControlBlock::new(&process, true));
    let mut new_thread_inner = new_thread.inner();
    let new_thread_ustack_top = new_thread_inner
        .res
        .as_ref()
        .unwrap()
        .user_stack_high_addr();
    let new_thread_tid = new_thread_inner.res.as_ref().unwrap().tid;
    let new_thread_trap_ctx = new_thread_inner.trap_ctx();
    *new_thread_trap_ctx = TrapContext::app_init_context(
        entry,
        new_thread_ustack_top,
        kernel_token(),
        new_thread.kernel_stack.top(),
        trap_handler as usize,
    );
    new_thread_trap_ctx.x[10] = arg;

    let mut inner = process.inner();
    let threads = &mut inner.threads;
    while threads.len() < new_thread_tid + 1 {
        threads.push(None);
    }
    threads[new_thread_tid] = Some(Arc::clone(&new_thread));
    add_task(Arc::clone(&new_thread));
    Ok(new_thread_tid as isize)
}

pub fn sys_gettid() -> Result {
    let tid = current_task().unwrap().inner().res.as_ref().unwrap().tid;
    Ok(tid as isize)
}

/// thread does not exist, return -1
/// thread has not exited yet, return -2
/// otherwise, return thread's exit code
pub fn sys_waittid(tid: usize) -> Result {
    let thread = current_task().unwrap();
    let process = thread.process.upgrade().unwrap();
    let thread_inner = thread.inner();
    let mut process_inner = process.inner();
    // a thread cannot wait for itself
    if thread_inner.res.as_ref().unwrap().tid == tid {
        return Err(code::TEMP);
    }
    let waited_thread = process_inner.thread_ref(tid).ok_or(code::TEMP)?;
    let exit_code = waited_thread.inner().exit_code.ok_or(code::TEMP)?;
    // dealloc the exited thread
    process_inner.threads[tid] = None;
    Ok(exit_code as isize)
}
