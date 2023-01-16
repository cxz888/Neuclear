pub const PTR_SIZE: usize = core::mem::size_of::<usize>();
pub const MEMORY_END: usize = 0x88000000;

pub const KERNEL_STACK_SIZE: usize = 4096 * 20;
pub const KERNEL_HEAP_SIZE: usize = 0x200_0000;

pub const PAGE_SIZE_BITS: usize = 0xc;
pub const PAGE_SIZE: usize = 1 << PAGE_SIZE_BITS;

pub const PTE_PER_PAGE: usize = PAGE_SIZE / PTR_SIZE;

pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;
pub const TRAP_CONTEXT: usize = TRAMPOLINE - PAGE_SIZE;
pub const USER_STACK: usize = TRAP_CONTEXT - USER_STACK_SIZE;
pub const USER_STACK_SIZE: usize = 4096 * 2;
/// 高 256GB 空间的起始点
pub const SECOND_START: usize = !((1 << 38) - 1);

pub const MAX_SYSCALL_NUM: usize = 500;

pub const CLOCK_FREQ: usize = 12500000;

pub const MMIO: &[(usize, usize)] = &[(0x10001000, 0x1000)];
