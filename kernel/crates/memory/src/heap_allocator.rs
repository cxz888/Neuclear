//! The global allocator

use buddy_system_allocator::LockedHeap;
use utils::config::KERNEL_HEAP_SIZE;

/// Heap allocator instance
#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<32> = LockedHeap::empty();

/// Panic when heap allocation error occurs
#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

/// heap space ([u8; KERNEL_HEAP_SIZE])
static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// 初始化内核堆，只应当调用一次
pub unsafe fn init_heap() {
    HEAP_ALLOCATOR
        .lock()
        .init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE);
}
