//! Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.

use super::__switch;
use super::process::ProcessControlBlock;
use super::{fetch_task, ThreadStatus};
use super::{TaskContext, ThreadControlBlock};
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;
use memory::PageTable;
use utils::upcell::UPSafeCell;

/// Processor management structure
pub struct Processor {
    /// The task currently executing on the current processor
    current: Option<Arc<ThreadControlBlock>>,
    /// The basic control flow of each core, helping to select and switch process
    idle_task_ctx: TaskContext,
}

impl Processor {
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_ctx: TaskContext::zero_init(),
        }
    }
    fn idle_task_ctx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_ctx as *mut _
    }
    pub fn take_current(&mut self) -> Option<Arc<ThreadControlBlock>> {
        self.current.take()
    }
    pub fn current(&self) -> Option<Arc<ThreadControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }
}

lazy_static! {
    /// 单处理器的实例，用于调度
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

/// The main part of process execution and scheduling
///
/// Loop fetch_task to get the process that needs to run,
/// and switch the process through __switch
pub fn run_tasks() -> ! {
    loop {
        let idle_task_ctx_ptr;
        let next_task_ctx_ptr;

        let task = fetch_task().expect("No more tasks avaliable.");

        // 准备新任务的 task ctx
        {
            let mut task_inner = task.inner();
            next_task_ctx_ptr = &task_inner.task_ctx as *const TaskContext;
            task_inner.thread_status = ThreadStatus::Running;
        }
        // 获取 idle 的 task ctx，同时将新任务放入处理器中
        {
            let process = task.process.upgrade().unwrap();
            process.inner().memory_set.activate();
            let mut processor = PROCESSOR.exclusive_access();
            processor.current = Some(task);
            idle_task_ctx_ptr = processor.idle_task_ctx_ptr();
        }

        unsafe {
            __switch(idle_task_ctx_ptr, next_task_ctx_ptr);
        };
    }
}

/// Get current task through take, leaving a None in its place
pub fn take_current_task() -> Option<Arc<ThreadControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

/// Get a copy of the current task
pub fn current_task() -> Option<Arc<ThreadControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

pub fn current_process() -> Arc<ProcessControlBlock> {
    current_task().unwrap().process.upgrade().unwrap()
}

/// 注意，会借用当前线程
#[track_caller]
pub fn current_page_table() -> PageTable {
    // TODO: 取得当前线程的 satp 可以直接读 satp 寄存器
    let task = current_task().unwrap();
    PageTable::from_token(task.user_token())
}

/// 需要保证 TrapContext 的引用 non-alias
#[track_caller]
pub unsafe fn current_trap_ctx() -> &'static mut TrapContext {
    current_task().unwrap().inner().trap_ctx()
}

/// 返回 idle 控制流（也就是 `run_tasks` 函数中），以便进行调度
pub fn schedule(switched_task_ctx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_ctx_ptr = processor.idle_task_ctx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_ctx_ptr, idle_task_ctx_ptr);
    }
}
