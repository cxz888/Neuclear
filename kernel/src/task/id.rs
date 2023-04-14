use super::ProcessControlBlock;
use crate::config::{
    KERNEL_STACK_SIZE, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT, USER_STACK, USER_STACK_SIZE,
};
use crate::memory::{MapPermission, MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::sync::UPSafeCell;
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};

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

/// Return (bottom, top) of a kernel stack in kernel space.
pub fn kernel_stack_position(kstack_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - kstack_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

/// 内核栈，进程在内核中进行处理时所使用的的栈，紧邻 TRAMPOLINE 正下方。
pub struct KernelStack(pub usize);

pub fn kstack_alloc() -> KernelStack {
    let kstack_id = KSTACK_ALLOCATOR.exclusive_access().alloc();
    let (kstack_bottom, kstack_top) = kernel_stack_position(kstack_id);
    //println!("kstack_alloc  kstack_bottom: {:#x?}, kstack_top: {:#x?}", kstack_bottom, kstack_top);
    KERNEL_SPACE.exclusive_access().insert_framed_area(
        VirtAddr(kstack_bottom).vpn_floor(),
        VirtAddr(kstack_top).vpn_ceil(),
        MapPermission::R | MapPermission::W,
    );
    KernelStack(kstack_id)
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        let (kernel_stack_bottom, _) = kernel_stack_position(self.0);
        let kernel_stack_bottom_va = VirtAddr(kernel_stack_bottom);
        // let kernel_stack_bottom_pa: PhysAddr = kernel_stack_bottom.into();
        // println!("kstack_drop  kstack_bottom: va: {:#x?}, pa: {:#x?}", kernel_stack_bottom_va, kernel_stack_bottom_pa);
        KERNEL_SPACE
            .exclusive_access()
            .remove_area_with_start_vpn(kernel_stack_bottom_va.vpn());
    }
}

impl KernelStack {
    #[allow(unused)]
    pub fn push_on_top<T>(&self, value: T) -> *mut T
    where
        T: Sized,
    {
        let kernel_stack_top = self.top();
        let ptr_mut = (kernel_stack_top - core::mem::size_of::<T>()) as *mut T;
        unsafe {
            *ptr_mut = value;
        }
        ptr_mut
    }
    pub fn top(&self) -> usize {
        let (_, kernel_stack_top) = kernel_stack_position(self.0);
        kernel_stack_top
    }
}

pub struct ThreadUserRes {
    pub tid: usize,
    pub process: Weak<ProcessControlBlock>,
}

impl ThreadUserRes {
    /// 用户资源包括用户栈和 trap_ctx。
    pub fn new(process: &Arc<ProcessControlBlock>, alloc_user_res: bool) -> Self {
        let tid = process.inner().alloc_tid();
        let thread_user_res = Self {
            tid,
            process: Arc::downgrade(process),
        };
        if alloc_user_res {
            thread_user_res.alloc_user_res(&mut process.inner().memory_set);
        }
        thread_user_res
    }

    /// 分配用户空间所需的资源，包括用户栈和 trap_ctx
    pub fn alloc_user_res(&self, memory_set: &mut MemorySet) {
        // alloc user stack
        let ustack_bottom = self.user_stack_low_addr();
        log::debug!("stack low addr: {:#x}", ustack_bottom);
        let ustack_top = ustack_bottom + USER_STACK_SIZE;
        log::debug!("stack high addr: {:#x}", ustack_top);
        memory_set.insert_framed_area(
            VirtAddr(ustack_bottom).vpn_floor(),
            VirtAddr(ustack_top).vpn_ceil(),
            MapPermission::R | MapPermission::W | MapPermission::U,
        );
        // alloc trap_ctx
        let trap_ctx_bottom = self.trap_ctx_low_addr();
        let trap_ctx_top = trap_ctx_bottom + PAGE_SIZE;
        memory_set.insert_framed_area(
            VirtAddr(trap_ctx_bottom).vpn_floor(),
            VirtAddr(trap_ctx_top).vpn_ceil(),
            MapPermission::R | MapPermission::W,
        );
    }

    fn dealloc_user_res(&self) {
        // dealloc tid
        let process = self.process.upgrade().unwrap();
        let mut inner = process.inner();
        // dealloc ustack manually
        let user_stack_low_addr = VirtAddr(self.user_stack_low_addr());
        inner
            .memory_set
            .remove_area_with_start_vpn(user_stack_low_addr.vpn());
        // dealloc trap_ctx manually
        let trap_ctx_low_addr = VirtAddr(self.trap_ctx_low_addr());
        inner
            .memory_set
            .remove_area_with_start_vpn(trap_ctx_low_addr.vpn());
    }

    pub fn dealloc_tid(&self) {
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner();
        process_inner.dealloc_tid(self.tid);
    }

    pub fn trap_ctx_ppn(&self, memory_set: &mut MemorySet) -> Option<PhysPageNum> {
        let trap_ctx_bottom_va = VirtAddr(self.trap_ctx_low_addr());
        memory_set.translate(trap_ctx_bottom_va.vpn())
    }

    pub fn trap_ctx_low_addr(&self) -> usize {
        // 一个用户栈，一个 Guard Page，一个 Trap Context
        TRAP_CONTEXT - self.tid * (USER_STACK_SIZE + PAGE_SIZE * 2)
    }
    pub fn user_stack_low_addr(&self) -> usize {
        USER_STACK - self.tid * (USER_STACK_SIZE + PAGE_SIZE * 2)
    }
    pub fn user_stack_high_addr(&self) -> usize {
        USER_STACK - self.tid * (USER_STACK_SIZE + PAGE_SIZE * 2) + USER_STACK_SIZE
    }
}

impl Drop for ThreadUserRes {
    fn drop(&mut self) {
        self.dealloc_tid();
        self.dealloc_user_res();
    }
}
