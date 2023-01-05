use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::block_current_and_run_next;
use crate::task::processor::{current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;

pub fn sys_sleep(ms: usize) -> isize {
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}

// LAB5 HINT: you might need to maintain data structures used for deadlock detection
// during sys_mutex_* and sys_semaphore_* syscalls
pub fn sys_mutex_create(blocking: bool) -> isize {
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    // NOTE: 有 create，但似乎没有销毁。那么是否意味着 mutex_list 里永远都是 Some
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        process_inner.mutex_available[id] = 1;
        // 清理工作不应该放在这里，如果有 create 对应的销毁那么就放在那里吧
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        for allocation in &mut process_inner.mutex_allocation {
            allocation.push(0);
        }
        for need in &mut process_inner.mutex_need {
            need.push(0);
        }
        process_inner.mutex_available.push(1);
        process_inner.mutex_list.len() as isize - 1
    }
}

// LAB5 HINT: Return -0xDEAD if deadlock is detected
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;

    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let mutex = Arc::clone(inner.mutex_list[mutex_id].as_ref().unwrap());
    inner.mutex_need[tid][mutex_id] += 1;
    if inner.detect_deadlock && !inner.check_mutex_safety() {
        return -0xDEAD;
    }
    drop(inner);
    drop(process);
    // lock 可能导致任务阻塞并切换，所以要记得把上述两个先 drop 了
    mutex.lock();
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    inner.mutex_need[tid][mutex_id] -= 1;
    inner.mutex_allocation[tid][mutex_id] += 1;
    inner.mutex_available[mutex_id] -= 1;
    0
}

pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let mutex = Arc::clone(inner.mutex_list[mutex_id].as_ref().unwrap());
    inner.mutex_allocation[tid][mutex_id] -= 1;
    inner.mutex_available[mutex_id] += 1;
    // NOTE: unlock 一般不导致阻塞吧？那么为什么要 drop 呢？
    drop(inner);
    drop(process);
    mutex.unlock();
    0
}

pub fn sys_semaphore_create(res_count: usize) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    if let Some(id) = inner
        .sem_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        inner.sem_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        inner.sem_available[id] = res_count;
        id as isize
    } else {
        inner
            .sem_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        for allocation in &mut inner.sem_allocation {
            allocation.push(0);
        }
        for need in &mut inner.sem_need {
            need.push(0);
        }
        inner.sem_available.push(res_count);
        inner.sem_list.len() as isize - 1
    }
}

pub fn sys_semaphore_up(sem_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;

    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let sem = Arc::clone(inner.sem_list[sem_id].as_ref().unwrap());
    inner.sem_allocation[tid][sem_id] -= 1;
    inner.sem_available[sem_id] += 1;
    drop(inner);
    drop(process);
    sem.up();
    0
}

// LAB5 HINT: Return -0xDEAD if deadlock is detected
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;

    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let sem = Arc::clone(inner.sem_list[sem_id].as_ref().unwrap());
    inner.sem_need[tid][sem_id] += 1;
    if inner.detect_deadlock && !inner.check_semaphore_safety() {
        return -0xDEAD;
    }
    drop(inner);
    drop(process);
    sem.down();
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    inner.sem_need[tid][sem_id] -= 1;
    inner.sem_allocation[tid][sem_id] += 1;
    inner.sem_available[sem_id] -= 1;
    0
}

pub fn sys_condvar_create(_arg: usize) -> isize {
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}

pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}

pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}

// LAB5 YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    if enabled != 0 && enabled != 1 {
        return -1;
    }
    current_process().inner_exclusive_access().detect_deadlock = enabled != 0;
    0
}
