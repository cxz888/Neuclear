#![no_std]
#![feature(step_trait)]
#![feature(alloc_error_handler)]
#![feature(assert_matches)]

extern crate alloc;

mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

pub use address::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
pub use frame_allocator::{frame_alloc, frame_dealloc, FrameTracker};
pub use memory_set::{kernel_token, MapArea, MapPermission, MapType, MemorySet, KERNEL_SPACE};
pub use page_table::{PTEFlags, PageTable, PageTableEntry, UserBuffer};

use utils::config::{PAGE_SIZE, PA_TO_VA};

#[inline]
#[track_caller]
pub fn kernel_va_to_pa(va: VirtAddr) -> PhysAddr {
    PhysAddr(va.0 - PA_TO_VA)
}

#[inline]
pub fn kernel_pa_to_va(pa: PhysAddr) -> VirtAddr {
    VirtAddr(pa.0 + PA_TO_VA)
}

#[inline]
pub fn kernel_ppn_to_vpn(ppn: PhysPageNum) -> VirtPageNum {
    VirtPageNum(ppn.0 + PA_TO_VA / PAGE_SIZE)
}

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    unsafe { heap_allocator::init_heap() };
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}
