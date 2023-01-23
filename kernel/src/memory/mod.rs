mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

pub use address::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
pub use frame_allocator::{frame_alloc, frame_dealloc, FrameTracker};
pub use memory_set::{kernel_token, MapArea, MapPermission, MapType, MemorySet, KERNEL_SPACE};
pub use page_table::{PTEFlags, PageTable, PageTableEntry, UserBuffer};

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    unsafe { heap_allocator::init_heap() };
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}
