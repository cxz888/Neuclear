use crate::trap::TrapContext;

use super::ProcessControlBlock;
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use memory::{MapPermission, MemorySet, VirtAddr, KERNEL_SPACE};
use utils::config::{ADDR_END, KERNEL_STACK_SIZE, LOW_END, PAGE_SIZE, USER_STACK_SIZE};
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
static KSTACK_ALLOCATOR: UPSafeCell<RecycleAllocator> =
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

/// 返回内核栈的在内核地址空间中的（低地址，高地址）
#[inline]
pub fn kernel_stack_addr(kstack_id: usize) -> (usize, usize) {
    // 每个内核栈下方紧跟着一个 Guard Page，用于防止溢出
    // 为了防止算术溢出，从倒数第二个页开始用作内核栈
    let high = ADDR_END - PAGE_SIZE - kstack_id * (KERNEL_STACK_SIZE + PAGE_SIZE) + 1;
    let low = high - KERNEL_STACK_SIZE;
    (low, high)
}

/// 内核栈，线程在内核中进行处理时所使用的的栈位于地址空间的最高处。
pub struct KernelStack(pub usize);

impl KernelStack {
    /// 返回内核空间中内核栈的高地址
    #[inline]
    pub fn high_addr(&self) -> usize {
        kernel_stack_addr(self.0).1
    }
    /// 返回内核空间中内核栈的 `TrapContext` 起始的位置，同时也是内核栈最初运行时实质上的栈顶
    #[inline]
    pub fn trap_ctx_addr(&self) -> usize {
        self.high_addr() - core::mem::size_of::<TrapContext>()
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let (kernel_stack_bottom, _) = kernel_stack_addr(self.0);
        let kernel_stack_bottom_va = VirtAddr(kernel_stack_bottom);
        // let kernel_stack_bottom_pa: PhysAddr = kernel_stack_bottom.into();
        // println!("kstack_drop  kstack_bottom: va: {:#x?}, pa: {:#x?}", kernel_stack_bottom_va, kernel_stack_bottom_pa);
        KSTACK_ALLOCATOR.exclusive_access().dealloc(self.0);
        KERNEL_SPACE
            .exclusive_access()
            .remove_area_with_start_vpn(kernel_stack_bottom_va.vpn());
    }
}

/// 给一个线程分配内核栈
pub fn kstack_alloc() -> KernelStack {
    let kstack_id = KSTACK_ALLOCATOR.exclusive_access().alloc();
    let (kstack_low, kstack_high) = kernel_stack_addr(kstack_id);
    // 内核中
    KERNEL_SPACE.exclusive_access().insert_framed_area(
        VirtAddr(kstack_low).vpn_floor(),
        VirtAddr(kstack_high).vpn_ceil(),
        MapPermission::R | MapPermission::W,
    );
    KernelStack(kstack_id)
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

    /// 获取当前线程用户栈的高地址，即低地址加上用户栈大小
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
