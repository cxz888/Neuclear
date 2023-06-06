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

        #[cfg(feature = "test")]
        let task = if let Some(existed_task) = fetch_task() {
            existed_task
        } else {
            while !super::check_timer() {}
            loop {
                let mut app_name = super::ALL_APPS
                    .exclusive_access()
                    .pop()
                    .expect("No more tasks available.");
                log::info!("next app: {app_name}");
                if super::INITPROC._spawn(app_name).is_ok() {
                    break;
                }
            }

            fetch_task().unwrap()
        };
        // 非初赛测试情况下，没有任务就可以退出操作系统了
        #[cfg(not(feature = "test"))]
        let task = fetch_task().expect("No more tasks available.");
        // log::info!("new task: {:?}", task.process.upgrade().unwrap().pid);

        // log::debug!("exec ok");

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
pub fn take_curr_task() -> Option<Arc<ThreadControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

/// Get a copy of the current task
pub fn curr_task() -> Option<Arc<ThreadControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

pub fn curr_process() -> Arc<ProcessControlBlock> {
    curr_task().unwrap().process.upgrade().unwrap()
}

#[track_caller]
pub fn curr_page_table() -> PageTable {
    // NOTE: 这里取得当前页表是直接读的 satp，是否要注意互斥之类的？
    let satp = riscv::register::satp::read();
    PageTable::from_token(satp.bits())
}

/// 需要保证 TrapContext 的引用 non-alias
#[track_caller]
pub unsafe fn curr_trap_ctx() -> &'static mut TrapContext {
    curr_task().unwrap().inner().trap_ctx()
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
