//! Implementation of [`FrameAllocator`] which
//! controls all the frames in the operating system.

use crate::PhysAddr;

use super::{kernel_ppn_to_vpn, kernel_va_to_pa, PhysPageNum, VirtAddr};
use alloc::vec::Vec;
use utils::{config::MEMORY_END, upcell::UPSafeCell};

/// manage a frame which has the same lifecycle as the tracker
#[derive(Debug)]
pub struct FrameTracker {
    pub ppn: PhysPageNum,
}

impl FrameTracker {
    // 分配一个新的物理帧，同时会将该物理帧清空
    pub fn new(ppn: PhysPageNum) -> Self {
        let mut frame = Self { ppn };
        frame.fill(0);
        frame
    }

    fn fill(&mut self, byte: u8) {
        let mut vpn = kernel_ppn_to_vpn(self.ppn);
        unsafe {
            let bytes = vpn.as_page_bytes_mut();
            bytes.fill(byte);
        }
    }
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        frame_dealloc(self.ppn);
    }
}

trait FrameAllocator {
    fn alloc(&mut self) -> Option<PhysPageNum>;
    fn dealloc(&mut self, ppn: PhysPageNum);
}

/// an implementation for frame allocator
pub struct StackFrameAllocator {
    current: PhysPageNum,
    end: PhysPageNum,
    recycled: Vec<PhysPageNum>,
}

impl StackFrameAllocator {
    const fn new() -> Self {
        Self {
            current: PhysPageNum(0),
            end: PhysPageNum(0),
            recycled: Vec::new(),
        }
    }
    pub fn init(&mut self, l: PhysPageNum, r: PhysPageNum) {
        self.current = l;
        self.end = r;
    }
}
impl FrameAllocator for StackFrameAllocator {
    /// 如果有回收的物理页，则出栈并返回。否则从区间左侧弹出。
    fn alloc(&mut self) -> Option<PhysPageNum> {
        if let Some(ppn) = self.recycled.pop() {
            Some(ppn)
        } else if self.current == self.end {
            None
        } else {
            self.current.0 += 1;
            Some(PhysPageNum(self.current.0 - 1))
        }
    }
    fn dealloc(&mut self, ppn: PhysPageNum) {
        let ppn = ppn;
        // validity check
        if ppn >= self.current || self.recycled.iter().any(|v| *v == ppn) {
            panic!("Frame ppn={:#x} has not been allocated!", ppn.0);
        }
        // recycle
        self.recycled.push(ppn);
    }
}

type FrameAllocatorImpl = StackFrameAllocator;

static FRAME_ALLOCATOR: UPSafeCell<FrameAllocatorImpl> =
    unsafe { UPSafeCell::new(FrameAllocatorImpl::new()) };

pub fn init_frame_allocator() {
    extern "C" {
        fn ekernel();
    }
    FRAME_ALLOCATOR.exclusive_access().init(
        kernel_va_to_pa(VirtAddr(ekernel as usize)).ceil(),
        PhysAddr(MEMORY_END).floor(),
    );
}

/// initiate the frame allocator using `ekernel` and `MEMORY_END`
pub fn frame_alloc() -> Option<FrameTracker> {
    FRAME_ALLOCATOR
        .exclusive_access()
        .alloc()
        .map(FrameTracker::new)
}

/// deallocate a frame
#[track_caller]
pub fn frame_dealloc(ppn: PhysPageNum) {
    FRAME_ALLOCATOR.exclusive_access().dealloc(ppn);
}
