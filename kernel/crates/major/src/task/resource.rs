use crate::trap::TrapContext;

use super::ProcessControlBlock;
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use memory::{frame_alloc, kernel_ppn_to_vpn, FrameTracker, MapPermission, MemorySet, VirtAddr};
use utils::config::{KERNEL_STACK_SIZE, LOW_END, PAGE_SIZE, USER_STACK_SIZE};
use utils::upcell::UPSafeCell;

#[derive(Clone)]
pub struct RecycleAllocator {
    current: usize,
    recycled: Vec<usize>,
}

impl RecycleAllocator {
    pub const fn new() -> Self {
        RecycleAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }
    pub fn alloc(&mut self) -> usize {
        if let Some(id) = self.recycled.pop() {
            id
        } else {
            self.current += 1;
            self.current - 1
        }
    }
    pub fn dealloc(&mut self, id: usize) {
        assert!(id < self.current);
        assert!(
            !self.recycled.iter().any(|i| *i == id),
            "id {} has been deallocated!",
            id
        );
        self.recycled.push(id);
    }
}

static PID_ALLOCATOR: UPSafeCell<RecycleAllocator> =
    unsafe { UPSafeCell::new(RecycleAllocator::new()) };

#[derive(Debug)]
pub struct PidHandle(pub usize);

pub fn pid_alloc() -> PidHandle {
    PidHandle(PID_ALLOCATOR.exclusive_access().alloc())
}

impl Drop for PidHandle {
    fn drop(&mut self) {
        PID_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}

/// 内核栈，线程在内核中进行处理时所使用的的栈位于地址空间的最高处。
pub struct KernelStack(pub FrameTracker);

impl KernelStack {
    /// 返回内核空间中内核栈的高地址
    #[inline]
    pub fn high_addr(&self) -> usize {
        kernel_ppn_to_vpn(self.0.ppn.add(self.0.num)).page_start().0
    }
    /// 返回内核空间中内核栈的 `TrapContext` 起始的位置，同时也是内核栈最初运行时实质上的栈顶
    #[inline]
    pub fn trap_ctx_addr(&self) -> usize {
        self.high_addr() - core::mem::size_of::<TrapContext>()
    }
}

/// 给一个线程分配内核栈
pub fn kstack_alloc() -> KernelStack {
    let kstack = KernelStack(frame_alloc(KERNEL_STACK_SIZE / PAGE_SIZE).unwrap());
    let kstack_high = kstack.high_addr();
    let kstack_low = kstack_high - KERNEL_STACK_SIZE;
    log::info!("Kernel stack [{kstack_low:#x},{kstack_high:#x})");
    kstack
}

/// 用户资源，目前也就是用户栈
pub struct ThreadUserRes {
    pub tid: usize,
    pub process: Weak<ProcessControlBlock>,
}

impl ThreadUserRes {
    // 仅分配 tid，后续的用户资源需要自行调用相关函数
    pub fn new(process: &Arc<ProcessControlBlock>) -> Self {
        let tid = process.inner().alloc_tid();
        let thread_user_res = Self {
            tid,
            process: Arc::downgrade(process),
        };
        thread_user_res
    }

    /// 分配用户空间所需的资源
    pub fn alloc_user_res(&self, memory_set: &mut MemorySet) {
        // 分配用户栈
        let ustack_bottom = self.user_stack_low_addr();
        log::debug!("stack low addr: {:#x}", ustack_bottom);
        let ustack_top = ustack_bottom + USER_STACK_SIZE;
        log::debug!("stack high addr: {:#x}", ustack_top);
        memory_set.insert_framed_area(
            VirtAddr(ustack_bottom).vpn_floor(),
            VirtAddr(ustack_top).vpn_ceil(),
            MapPermission::R | MapPermission::W | MapPermission::U,
        );
    }

    /// 释放用户资源
    fn dealloc_user_res(&self) {
        let process = self.process.upgrade().unwrap();
        let mut inner = process.inner();
        // 手动取消用户栈的映射
        let user_stack_low_addr = VirtAddr(self.user_stack_low_addr());
        inner
            .memory_set
            .remove_area_with_start_vpn(user_stack_low_addr.vpn());
    }

    /// 释放用户线程的 tid
    pub fn dealloc_tid(&self) {
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner();
        process_inner.dealloc_tid(self.tid);
    }

    /// 获取当前线程用户栈的低地址，即高地址减去用户栈大小
    #[inline]
    pub fn user_stack_low_addr(&self) -> usize {
        self.user_stack_high_addr() - USER_STACK_SIZE
    }

    /// 获取当前线程用户栈的高地址
    #[inline]
    pub fn user_stack_high_addr(&self) -> usize {
        // 注意每个用户栈后都会有一个 Guard Page
        LOW_END - self.tid * (USER_STACK_SIZE + PAGE_SIZE)
    }
}

impl Drop for ThreadUserRes {
    fn drop(&mut self) {
        self.dealloc_tid();
        self.dealloc_user_res();
    }
}
