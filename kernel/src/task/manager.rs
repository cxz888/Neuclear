//! Implementation of [`TaskManager`]
//!
//! It is only used to manage processes and schedule process based on ready queue.
//! Other CPU process monitoring functions are in Processor.

use super::{ThreadControlBlock, INITPROC};
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::lazy_static;

pub struct TaskManager {
    ready_queue: VecDeque<Arc<ThreadControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    pub const fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    fn add(&mut self, task: Arc<ThreadControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    fn fetch(&mut self) -> Option<Arc<ThreadControlBlock>> {
        self.ready_queue.pop_front()
    }
}

lazy_static! {
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> = unsafe {
        let mut task_manager = TaskManager::new();
        task_manager.add(INITPROC.inner().main_thread());
        UPSafeCell::new(task_manager)
    };
}

pub fn add_task(task: Arc<ThreadControlBlock>) {
    TASK_MANAGER.exclusive_access().add(task);
}

pub fn fetch_task() -> Option<Arc<ThreadControlBlock>> {
    TASK_MANAGER.exclusive_access().fetch()
}
