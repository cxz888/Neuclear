use super::{ProcessControlBlock, ProcessControlBlockInner};
use crate::config::{KERNEL_STACK_SIZE, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT, USER_STACK_SIZE};
use crate::mm::{MapPermission, MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE};
use crate::sync::UPSafeCell;
use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use lazy_static::*;

pub struct RecycleAllocator {
    current: usize,
    recycled: Vec<usize>,
}

impl RecycleAllocator {
    pub fn new() -> Self {
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

lazy_static! {
    static ref PID_ALLOCATOR: UPSafeCell<RecycleAllocator> =
        unsafe { UPSafeCell::new(RecycleAllocator::new()) };
    static ref KSTACK_ALLOCATOR: UPSafeCell<RecycleAllocator> =
        unsafe { UPSafeCell::new(RecycleAllocator::new()) };
}

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

pub struct KernelStack(pub usize);

pub fn kstack_alloc() -> KernelStack {
    let kstack_id = KSTACK_ALLOCATOR.exclusive_access().alloc();
    let (kstack_bottom, kstack_top) = kernel_stack_position(kstack_id);
    //println!("kstack_alloc  kstack_bottom: {:#x?}, kstack_top: {:#x?}", kstack_bottom, kstack_top);
    KERNEL_SPACE.exclusive_access().insert_framed_area(
        VirtAddr(kstack_bottom),
        VirtAddr(kstack_top),
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

pub struct TaskUserRes {
    pub tid: usize,
    pub ustack_base: usize,
    pub process: Weak<ProcessControlBlock>,
}

fn trap_cx_bottom_from_tid(tid: usize) -> usize {
    TRAP_CONTEXT - tid * PAGE_SIZE
}

fn ustack_bottom_from_tid(ustack_base: usize, tid: usize) -> usize {
    ustack_base + tid * (PAGE_SIZE + USER_STACK_SIZE)
}

impl TaskUserRes {
    pub fn new(
        process: &Arc<ProcessControlBlock>,
        ustack_base: usize,
        alloc_user_res: bool,
    ) -> Self {
        let tid = process.inner_exclusive_access().alloc_tid();
        let task_user_res = Self {
            tid,
            ustack_base,
            process: Arc::downgrade(process),
        };
        if alloc_user_res {
            task_user_res.alloc_user_res(&mut process.inner_exclusive_access().memory_set);
        }
        task_user_res
    }

    /// 分配用户空间所需的资源，包括用户栈和 trap_ctx
    pub fn alloc_user_res(&self, memory_set: &mut MemorySet) {
        // alloc user stack
        let ustack_bottom = ustack_bottom_from_tid(self.ustack_base, self.tid);
        log::debug!("stack low addr: {:#x}", ustack_bottom);
        let ustack_top = ustack_bottom + USER_STACK_SIZE;
        log::debug!("stack high addr: {:#x}", ustack_top);
        memory_set.insert_framed_area(
            VirtAddr(ustack_bottom),
            VirtAddr(ustack_top),
            MapPermission::R | MapPermission::W | MapPermission::U,
        );
        // alloc trap_ctx
        let trap_ctx_bottom = trap_cx_bottom_from_tid(self.tid);
        let trap_ctx_top = trap_ctx_bottom + PAGE_SIZE;
        memory_set.insert_framed_area(
            VirtAddr(trap_ctx_bottom),
            VirtAddr(trap_ctx_top),
            MapPermission::R | MapPermission::W,
        );
    }

    fn dealloc_user_res(&self) {
        // dealloc tid
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner_exclusive_access();
        // dealloc ustack manually
        let ustack_bottom_va = VirtAddr(ustack_bottom_from_tid(self.ustack_base, self.tid));
        process_inner
            .memory_set
            .remove_area_with_start_vpn(ustack_bottom_va.vpn());
        // dealloc trap_cx manually
        let trap_cx_bottom_va = VirtAddr(trap_cx_bottom_from_tid(self.tid));
        process_inner
            .memory_set
            .remove_area_with_start_vpn(trap_cx_bottom_va.vpn());
    }

    #[allow(unused)]
    pub fn alloc_tid(&mut self) {
        self.tid = self
            .process
            .upgrade()
            .unwrap()
            .inner_exclusive_access()
            .alloc_tid();
    }

    pub fn dealloc_tid(&self) {
        let process = self.process.upgrade().unwrap();
        let mut process_inner = process.inner_exclusive_access();
        process_inner.dealloc_tid(self.tid);
    }

    pub fn trap_cx_user_va(&self) -> usize {
        trap_cx_bottom_from_tid(self.tid)
    }

    pub fn trap_ctx_ppn(&self, pcb: &mut ProcessControlBlockInner) -> PhysPageNum {
        let trap_cx_bottom_va = VirtAddr(trap_cx_bottom_from_tid(self.tid));
        pcb.memory_set
            .translate(trap_cx_bottom_va.vpn())
            .unwrap()
            .ppn()
    }

    pub fn ustack_base(&self) -> usize {
        self.ustack_base
    }
    pub fn ustack_top(&self) -> usize {
        ustack_bottom_from_tid(self.ustack_base, self.tid) + USER_STACK_SIZE
    }
}

impl Drop for TaskUserRes {
    fn drop(&mut self) {
        self.dealloc_tid();
        self.dealloc_user_res();
    }
}

use alloc::alloc::{alloc, dealloc, Layout};

#[derive(Clone)]
pub struct KStack(usize);

const STACK_SIZE: usize = 0x8000;

impl KStack {
    pub fn new() -> KStack {
        let bottom =
            unsafe { alloc(Layout::from_size_align(STACK_SIZE, STACK_SIZE).unwrap()) } as usize;
        KStack(bottom)
    }

    pub fn top(&self) -> usize {
        self.0 + STACK_SIZE
    }
}
use core::fmt::{self, Debug, Formatter};
impl Debug for KStack {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("KStack:{:#x}", self.0))
    }
}

impl Drop for KStack {
    fn drop(&mut self) {
        unsafe {
            dealloc(
                self.0 as _,
                Layout::from_size_align(STACK_SIZE, STACK_SIZE).unwrap(),
            );
        }
    }
}
