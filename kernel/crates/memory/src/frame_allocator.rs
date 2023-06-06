//! Implementation of [`FrameAllocator`] which
//! controls all the frames in the operating system.

use core::ops::Range;

use crate::PhysAddr;

use super::{kernel_ppn_to_vpn, kernel_va_to_pa, PhysPageNum, VirtAddr};
use utils::{
    config::{MEMORY_END, MEMORY_SIZE, PAGE_SIZE},
    upcell::UPSafeCell,
};

/// manage a frame which has the same lifecycle as the tracker
#[derive(Debug)]
pub struct FrameTracker {
    pub ppn: PhysPageNum,
    pub num: usize,
}

impl FrameTracker {
    // 分配一个新的物理帧，同时会将该物理帧清空
    pub fn new(ppn: PhysPageNum, num: usize) -> Self {
        assert!(num >= 1);
        let mut frame = Self { ppn, num };
        frame.fill(0);
        frame
    }

    fn fill(&mut self, byte: u8) {
        let va = kernel_ppn_to_vpn(self.ppn).page_start();
        unsafe {
            let bytes = core::slice::from_raw_parts_mut(va.0 as _, self.num * PAGE_SIZE);
            bytes.fill(byte);
        }
    }
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        frame_dealloc(self.ppn..(PhysPageNum(self.ppn.0 + self.num)));
    }
}

trait FrameAllocator {
    fn alloc(&mut self, num: usize) -> Option<PhysPageNum>;
    fn dealloc(&mut self, range: Range<PhysPageNum>);
}

/// an implementation for frame allocator
// pub struct StackFrameAllocator {
//     current: PhysPageNum,
//     end: PhysPageNum,
//     recycled: Vec<PhysPageNum>,
// }

// impl StackFrameAllocator {
//     const fn new() -> Self {
//         Self {
//             current: PhysPageNum(0),
//             end: PhysPageNum(0),
//             recycled: Vec::new(),
//         }
//     }
//     pub fn init(&mut self, l: PhysPageNum, r: PhysPageNum) {
//         self.current = l;
//         self.end = r;
//     }
// }

// impl FrameAllocator for StackFrameAllocator {
//     /// 如果有回收的物理页，则出栈并返回。否则从区间左侧弹出。
//     fn alloc(&mut self) -> Option<PhysPageNum> {
//         if let Some(ppn) = self.recycled.pop() {
//             Some(ppn)
//         } else if self.current == self.end {
//             None
//         } else {
//             self.current.0 += 1;
//             Some(PhysPageNum(self.current.0 - 1))
//         }
//     }
//     fn dealloc(&mut self, ppn: PhysPageNum) {
//         let ppn = ppn;
//         // validity check
//         if ppn >= self.current || self.recycled.iter().any(|v| *v == ppn) {
//             panic!("Frame ppn={:#x} has not been allocated!", ppn.0);
//         }
//         // recycle
//         self.recycled.push(ppn);
//     }
// }

const BUDDY_ORDER: usize = ((MEMORY_SIZE - 1) / PAGE_SIZE).ilog2() as usize + 1;

pub struct BuddySystemFrameAllocator {
    allocator: buddy_system_allocator::FrameAllocator<BUDDY_ORDER>,
}

impl BuddySystemFrameAllocator {
    pub const fn new() -> Self {
        Self {
            allocator: buddy_system_allocator::FrameAllocator::new(),
        }
    }
}

extern "C" {
    fn ekernel();
}

impl FrameAllocator for BuddySystemFrameAllocator {
    fn alloc(&mut self, num: usize) -> Option<PhysPageNum> {
        let physical_memory_begin_frame: usize =
            kernel_va_to_pa(VirtAddr(ekernel as usize)).ceil().0;
        self.allocator
            .alloc(num)
            .map(|first| PhysPageNum(first + physical_memory_begin_frame))
    }

    fn dealloc(&mut self, range: Range<PhysPageNum>) {
        self.allocator.dealloc(range.start.0, range.end.0);
    }
}

type FrameAllocatorImpl = BuddySystemFrameAllocator;

static FRAME_ALLOCATOR: UPSafeCell<FrameAllocatorImpl> =
    unsafe { UPSafeCell::new(FrameAllocatorImpl::new()) };

pub fn init_frame_allocator() {
    let physical_memory_begin_frame = kernel_va_to_pa(VirtAddr(ekernel as usize)).ceil().0;
    FRAME_ALLOCATOR.exclusive_access().allocator.add_frame(
        0,
        PhysAddr(MEMORY_END).floor().0 - physical_memory_begin_frame,
    );
}

/// initiate the frame allocator using `ekernel` and `MEMORY_END`
pub fn frame_alloc(num: usize) -> Option<FrameTracker> {
    FRAME_ALLOCATOR
        .exclusive_access()
        .alloc(num)
        .map(|ppn| FrameTracker::new(ppn, num))
}

/// deallocate a frame
#[track_caller]
pub fn frame_dealloc(range: Range<PhysPageNum>) {
    FRAME_ALLOCATOR.exclusive_access().dealloc(range);
}
