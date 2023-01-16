//! Types related to task management & Functions for completely changing TCB

use super::id::TaskUserRes;
use super::{kstack_alloc, KernelStack, ProcessControlBlock, TaskContext};
use crate::trap::TrapContext;
use crate::{mm::PhysPageNum, sync::UPSafeCell};
use alloc::sync::{Arc, Weak};
use core::cell::RefMut;

/// Task control block structure
///
/// Directly save the contents that will not change during running
pub struct TaskControlBlock {
    // immutable
    pub process: Weak<ProcessControlBlock>,
    /// Kernel stack corresponding to TID
    pub kernel_stack: KernelStack,
    // mutable
    inner: UPSafeCell<TaskControlBlockInner>,
}

/// Structure containing more process content
///
/// Store the contents that will change during operation
/// and are wrapped by UPSafeCell to provide mutual exclusion
pub struct TaskControlBlockInner {
    /// The physical page number of the frame where the trap context is placed
    pub trap_ctx_ppn: PhysPageNum,
    /// Save task context
    pub task_ctx: TaskContext,
    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,
    /// It is set when active exit or execution error occurs
    pub exit_code: Option<i32>,
    /// Tid and ustack will be deallocated when this goes None
    pub res: Option<TaskUserRes>,
    /// 实际上是 `*const i32`，因为裸指针不 `Send` 就用 `usize` 了
    pub clear_child_tid: usize,
}

/// Simple access to its internal fields
impl TaskControlBlockInner {
    pub fn trap_ctx(&mut self) -> &'static mut TrapContext {
        self.trap_ctx_ppn.as_mut()
    }

    #[allow(unused)]
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
}

impl TaskControlBlock {
    pub fn new(process: &Arc<ProcessControlBlock>, alloc_user_res: bool) -> Self {
        let res = TaskUserRes::new(process, alloc_user_res);
        let trap_ctx_ppn = res.trap_ctx_ppn(&mut process.inner_exclusive_access());
        let kernel_stack = kstack_alloc();
        let kstack_top = kernel_stack.top();
        Self {
            process: Arc::downgrade(process),
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    res: Some(res),
                    trap_ctx_ppn,
                    task_ctx: TaskContext::goto_trap_return(kstack_top),
                    task_status: TaskStatus::Ready,
                    exit_code: None,
                    clear_child_tid: 0,
                })
            },
        }
    }

    /// Get the mutex to get the RefMut TaskControlBlockInner
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }

    pub fn user_token(&self) -> usize {
        let process = self.process.upgrade().unwrap();
        let inner = process.inner_exclusive_access();
        inner.memory_set.token()
    }
}

/// task status: UnInit, Ready, Running, Exited
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    _UnInit,
    Ready,
    Running,
    Blocking,
}
