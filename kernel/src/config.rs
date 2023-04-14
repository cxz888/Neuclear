pub const PTR_SIZE: usize = core::mem::size_of::<usize>();
pub const MEMORY_END: usize = 0x8800_0000;

const KB: usize = 1024;
const MB: usize = 1024 * 1024;

/// 内核地址空间中，虚拟地址相对于物理地址的偏移量
pub const PA_TO_VA: usize = 0xFFFF_FFFF_0000_0000;

pub const KERNEL_STACK_SIZE: usize = 80 * KB;
pub const KERNEL_HEAP_SIZE: usize = 32 * MB;

pub const PAGE_SIZE_BITS: usize = 12;
pub const PAGE_SIZE: usize = 1 << PAGE_SIZE_BITS;

pub const PTE_PER_PAGE: usize = PAGE_SIZE / PTR_SIZE;

pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;
pub const TRAP_CONTEXT: usize = TRAMPOLINE - PAGE_SIZE;
pub const USER_STACK: usize = TRAP_CONTEXT - USER_STACK_SIZE;
pub const USER_STACK_SIZE: usize = 8 * KB;

/// mmap 开始寻找可映射段的起点，即低地址的 128GiB 处
pub const MMAP_START: usize = 0x20_0000_0000;
/// 低地址的末端，即 256GiB 处
pub const LOW_END: usize = 0x40_0000_0000;

pub const CLOCK_FREQ: usize = 12_500_000;

/// I/O 映射的起始地址和长度
pub const MMIO: &[(usize, usize)] = &[(0x1000_1000, 0x1000)];

/// 信号机制所需的 bitset 大小
pub const SIGSET_SIZE: usize = 64;
pub const SIGSET_SIZE_BYTES: usize = SIGSET_SIZE / 8;
