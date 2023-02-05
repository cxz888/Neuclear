//! Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.

use super::__switch;
use super::process::ProcessControlBlock;
use super::{fetch_task, ThreadStatus};
use super::{TaskContext, ThreadControlBlock};
use crate::memory::PageTable;
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;

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
    fn get_idle_task_ctx_ptr(&mut self) -> *mut TaskContext {
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
    /// PROCESSOR instance through lazy_static!
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

/// The main part of process execution and scheduling
///
/// Loop fetch_task to get the process that needs to run,
/// and switch the process through __switch
pub fn run_tasks() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(task) = fetch_task() {
            // println!("task get!");
            let idle_task_ctx_ptr = processor.get_idle_task_ctx_ptr();
            // access coming task TCB exclusively
            let mut task_inner = task.inner();
            let next_task_ctx_ptr = &task_inner.task_ctx as *const TaskContext;
            task_inner.thread_status = ThreadStatus::Running;
            drop(task_inner);
            // release coming task TCB manually
            processor.current = Some(task);
            // release processor manually
            drop(processor);
            unsafe {
                __switch(idle_task_ctx_ptr, next_task_ctx_ptr);
            }
        } else {
            panic!("no tasks available in run_tasks");
        }
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

/// Get token of the address space of current task
#[track_caller]
pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    task.user_token()
}

/// 注意，会借用当前线程
#[track_caller]
pub fn current_page_table() -> PageTable {
    let task = current_task().unwrap();
    PageTable::from_token(task.user_token())
}

/// Get the mutable reference to trap context of current task
#[track_caller]
pub fn current_trap_ctx() -> &'static mut TrapContext {
    current_task().unwrap().inner().trap_ctx()
}

#[track_caller]
pub fn current_trap_ctx_user_va() -> usize {
    current_task()
        .unwrap()
        .inner()
        .res
        .as_ref()
        .unwrap()
        .trap_ctx_low_addr()
}

/// Return to idle control flow for new scheduling
pub fn schedule(switched_task_ctx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_ctx_ptr = processor.get_idle_task_ctx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_ctx_ptr, idle_task_ctx_ptr);
    }
}
