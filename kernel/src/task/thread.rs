//! Types related to thread management & Functions for completely changing TCB

use super::id::ThreadUserRes;
use super::{kstack_alloc, KernelStack, ProcessControlBlock, TaskContext};
use crate::signal::SignalReceiver;
use crate::trap::TrapContext;
use crate::{memory::PhysPageNum, sync::UPSafeCell};
use alloc::sync::{Arc, Weak};
use core::cell::RefMut;

/// Thread control block structure
///
/// Directly save the contents that will not change during running
pub struct ThreadControlBlock {
    pub process: Weak<ProcessControlBlock>,
    /// Kernel stack corresponding to TID
    pub kernel_stack: KernelStack,
    tcb_inner: UPSafeCell<ThreadControlBlockInner>,
}

/// Structure containing more process content
///
/// Store the contents that will change during operation
/// and are wrapped by UPSafeCell to provide mutual exclusion
pub struct ThreadControlBlockInner {
    /// The physical page number of the frame where the trap context is placed
    pub trap_ctx_ppn: PhysPageNum,
    /// Save task context
    pub task_ctx: TaskContext,
    /// Maintain the execution status of the current process
    pub thread_status: ThreadStatus,
    /// It is set when active exit or execution error occurs
    pub exit_code: Option<i32>,
    /// Tid and ustack will be deallocated when this goes None
    pub res: Option<ThreadUserRes>,
    /// 实际上是 `*const i32`，因为裸指针不 `Send` 就用 `usize` 了
    pub clear_child_tid: usize,
    pub sig_receiver: SignalReceiver,
}

/// Simple access to its internal fields
impl ThreadControlBlockInner {
    /// TCB 的 trap_ctx_ppn 在正常情况下都是合法的，所以 safe
    pub fn trap_ctx(&mut self) -> &'static mut TrapContext {
        unsafe { self.trap_ctx_ppn.page_start().as_mut() }
    }

    #[allow(unused)]
    fn get_status(&self) -> ThreadStatus {
        self.thread_status
    }
}

impl ThreadControlBlock {
    /// 创建一个新的 TCB。初始的 TaskContext 返回到 `trap_return` 处
    pub fn new(process: &Arc<ProcessControlBlock>, alloc_user_res: bool) -> Self {
        let res = ThreadUserRes::new(process, alloc_user_res);
        // 如果这个为 0 说明用户资源暂时未分配。应当延后分配，比如 Load ELF 时
        let trap_ctx_ppn = res
            .trap_ctx_ppn(&mut process.inner().memory_set)
            .unwrap_or(PhysPageNum(0));
        let kernel_stack = kstack_alloc();
        let kstack_top = kernel_stack.top();
        Self {
            process: Arc::downgrade(process),
            kernel_stack,
            tcb_inner: unsafe {
                UPSafeCell::new(ThreadControlBlockInner {
                    res: Some(res),
                    trap_ctx_ppn,
                    task_ctx: TaskContext::trap_return_ctx(kstack_top),
                    thread_status: ThreadStatus::Ready,
                    exit_code: None,
                    clear_child_tid: 0,
                    sig_receiver: SignalReceiver::new(),
                })
            },
        }
    }

    #[track_caller]
    pub fn inner(&self) -> RefMut<'_, ThreadControlBlockInner> {
        self.tcb_inner.exclusive_access()
    }

    #[track_caller]
    pub fn user_token(&self) -> usize {
        let process = self.process.upgrade().unwrap();
        let inner = process.inner();
        inner.memory_set.token()
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ThreadStatus {
    Ready,
    Running,
    Blocking,
}
