use crate::{
    error::Result,
    sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore},
    task::{block_current_and_run_next, current_process, current_task},
    timer::{add_timer, get_time_ms},
};
use alloc::sync::Arc;

pub fn sys_sleep(ms: usize) -> Result {
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    Ok(0)
}

pub fn sys_mutex_create(blocking: bool) -> Result {
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
        // 清理工作不应该放在这里，如果有 create 对应的销毁那么就放在那里吧
        Ok(id as isize)
    } else {
        process_inner.mutex_list.push(mutex);
        Ok(process_inner.mutex_list.len() as isize - 1)
    }
}

pub fn sys_mutex_lock(mutex_id: usize) -> Result {
    let process = current_process();
    let inner = process.inner_exclusive_access();
    let mutex = Arc::clone(inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(inner);
    drop(process);
    // lock 可能导致任务阻塞并切换，所以要记得把上述两个先 drop 了
    mutex.lock();
    Ok(0)
}

pub fn sys_mutex_unlock(mutex_id: usize) -> Result {
    let process = current_process();
    let inner = process.inner_exclusive_access();
    let mutex = Arc::clone(inner.mutex_list[mutex_id].as_ref().unwrap());
    // NOTE: unlock 一般不导致阻塞吧？那么为什么要 drop 呢？
    drop(inner);
    drop(process);
    mutex.unlock();
    Ok(0)
}

pub fn sys_semaphore_create(res_count: usize) -> Result {
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
        Ok(id as isize)
    } else {
        inner
            .sem_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        Ok(inner.sem_list.len() as isize - 1)
    }
}

pub fn sys_semaphore_up(sem_id: usize) -> Result {
    let process = current_process();
    let inner = process.inner_exclusive_access();
    let sem = Arc::clone(inner.sem_list[sem_id].as_ref().unwrap());
    drop(inner);
    sem.up();
    Ok(0)
}

// LAB5 HINT: Return -0xDEAD if deadlock is detected
pub fn sys_semaphore_down(sem_id: usize) -> Result {
    let process = current_process();
    let inner = process.inner_exclusive_access();
    let sem = Arc::clone(inner.sem_list[sem_id].as_ref().unwrap());

    drop(inner);
    drop(process);
    sem.down();
    Ok(0)
}

pub fn sys_condvar_create(_arg: usize) -> Result {
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
    Ok(id as isize)
}

pub fn sys_condvar_signal(condvar_id: usize) -> Result {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    Ok(0)
}

pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> Result {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    Ok(0)
}
