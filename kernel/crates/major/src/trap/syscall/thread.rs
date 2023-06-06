use utils::error::{code, Result};

use crate::task::curr_task;

/// TODO: 写注释
pub fn sys_gettid() -> Result {
    let tid = curr_task().unwrap().inner().res.as_ref().unwrap().tid;
    Ok(tid as isize)
}

/// thread does not exist, return -1
/// thread has not exited yet, return -2
/// otherwise, return thread's exit code
pub fn sys_waittid(tid: usize) -> Result {
    let thread = curr_task().unwrap();
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
