use crate::{
    mm::kernel_token,
    task::{add_task, processor::current_task, TaskControlBlock},
    trap::{trap_handler, TrapContext},
};
use alloc::{sync::Arc, vec};

pub fn sys_thread_create(entry: usize, arg: usize) -> isize {
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    // create a new thread
    let new_task = Arc::new(TaskControlBlock::new(
        &process,
        task.inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .ustack_base,
        true,
    ));
    let mut new_task_inner = new_task.inner_exclusive_access();
    let new_task_ustack_top = new_task_inner.res.as_ref().unwrap().ustack_top();
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
    let mutex_len = inner.mutex_list.len();
    while inner.mutex_allocation.len() < new_task_tid + 1 {
        inner.mutex_allocation.push(vec![0; mutex_len])
    }
    while inner.mutex_need.len() < new_task_tid + 1 {
        inner.mutex_need.push(vec![0; mutex_len])
    }
    assert!(inner.mutex_allocation[new_task_tid]
        .iter()
        .all(|elem| *elem == 0));
    assert!(inner.mutex_need[new_task_tid].iter().all(|elem| *elem == 0));
    let sem_len = inner.sem_list.len();
    while inner.sem_allocation.len() < new_task_tid + 1 {
        inner.sem_allocation.push(vec![0; sem_len])
    }
    while inner.sem_need.len() < new_task_tid + 1 {
        inner.sem_need.push(vec![0; sem_len])
    }
    assert!(inner.sem_allocation[new_task_tid]
        .iter()
        .all(|elem| *elem == 0));
    assert!(inner.sem_need[new_task_tid].iter().all(|elem| *elem == 0));
    // add new thread to current process
    let tasks = &mut inner.tasks;
    while tasks.len() < new_task_tid + 1 {
        tasks.push(None);
    }
    tasks[new_task_tid] = Some(Arc::clone(&new_task));
    // add new task to scheduler
    add_task(Arc::clone(&new_task));
    new_task_tid as isize
}

pub fn sys_gettid() -> isize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid as isize
}

/// thread does not exist, return -1
/// thread has not exited yet, return -2
/// otherwise, return thread's exit code
pub fn sys_waittid(tid: usize) -> i32 {
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    let task_inner = task.inner_exclusive_access();
    let mut inner = process.inner_exclusive_access();
    // a thread cannot wait for itself
    if task_inner.res.as_ref().unwrap().tid == tid {
        return -1;
    }
    let mut exit_code: Option<i32> = None;
    let waited_task = inner.tasks[tid].as_ref();
    if let Some(waited_task) = waited_task {
        if let Some(waited_exit_code) = waited_task.inner_exclusive_access().exit_code {
            exit_code = Some(waited_exit_code);
        }
    } else {
        // waited thread does not exist
        return -1;
    }
    if let Some(exit_code) = exit_code {
        // dealloc the exited thread
        inner.mutex_allocation[tid].clear();
        inner.mutex_need[tid].clear();
        inner.sem_allocation[tid].clear();
        inner.sem_need[tid].clear();
        inner.tasks[tid] = None;
        exit_code
    } else {
        // waited thread has not exited
        -2
    }
}
