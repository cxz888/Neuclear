//! Types related to thread management & Functions for completely changing TCB

use super::resource::{kstack_alloc, ThreadUserRes};
use super::{KernelStack, ProcessControlBlock, TaskContext};
use crate::trap::TrapContext;
use alloc::sync::{Arc, Weak};
use core::cell::RefMut;
use memory::VirtAddr;
use signal::SignalReceiver;
use utils::upcell::UPSafeCell;

/// 进程控制块
///
/// TODO: tid 一般而言是不变的，那么是否可以放在这个结构里，而非在 Inner 中和 res 绑一块？目前是因为用户栈的位置取决于 tid 所以放在 res 里
pub struct ThreadControlBlock {
    pub process: Weak<ProcessControlBlock>,
    tcb_inner: UPSafeCell<ThreadControlBlockInner>,
}

impl ThreadControlBlock {
    /// 创建一个新的 TCB。初始的 TaskContext 返回到 `trap_return` 处。对应的内核栈也会在此函数中分配
    ///
    /// 注，暂时没有分配用户资源，需要后续手动分配
    pub fn new(process: &Arc<ProcessControlBlock>) -> Self {
        let res = ThreadUserRes::new(process);
        let kernel_stack = kstack_alloc();
        let kstack_ptr = kernel_stack.trap_ctx_addr();
        Self {
            process: Arc::downgrade(process),
            tcb_inner: unsafe {
                UPSafeCell::new(ThreadControlBlockInner {
                    kernel_stack,
                    res: Some(res),
                    task_ctx: TaskContext::goto_restore(kstack_ptr),
                    thread_status: ThreadStatus::Ready,
                    exit_code: None,
                    clear_child_tid: 0,
                    sig_receiver: SignalReceiver::new(),
                })
            },
        }
    }

    /// fork 时用到，从一个已有的 tcb 中复制出来一个。
    ///
    /// 但是内核栈会重新申请
    pub fn from_existed(other: &Self, process: &Arc<ProcessControlBlock>) -> Self {
        let other_inner = other.inner();
        let res = Some(ThreadUserRes {
            tid: other_inner.res.as_ref().unwrap().tid,
            process: Arc::downgrade(process),
        });
        let kernel_stack = kstack_alloc();
        let kstack_ptr = kernel_stack.trap_ctx_addr();
        Self {
            process: Arc::downgrade(process),
            tcb_inner: unsafe {
                UPSafeCell::new(ThreadControlBlockInner {
                    kernel_stack,
                    res,
                    task_ctx: TaskContext::goto_restore(kstack_ptr),
                    thread_status: ThreadStatus::Ready,
                    exit_code: None,
                    // TODO: 这些东西要不要复制？待考虑
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
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ThreadStatus {
    Ready,
    Running,
    Blocking,
}

pub struct ThreadControlBlockInner {
    /// 对应于 Tid 的内核栈
    pub kernel_stack: KernelStack,
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

impl ThreadControlBlockInner {
    /// 线程创建之后，内核栈一定是存在的，但因为仍需自己保证 non-alias，所以标为 `unsafe`
    pub unsafe fn trap_ctx(&mut self) -> &'static mut TrapContext {
        VirtAddr(self.kernel_stack.trap_ctx_addr()).as_mut()
    }

    #[allow(unused)]
    fn get_status(&self) -> ThreadStatus {
        self.thread_status
    }
}
