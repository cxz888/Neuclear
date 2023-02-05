use bitflags::bitflags;

bitflags! {
    pub struct MmapProt: u32 {
        const PROT_NONE  = 0;
        const PROT_READ  = 1 << 0;
        const PROT_WRITE = 1 << 1;
        const PROT_EXEC  = 1 << 2;
    }

    /// `MAP_SHARED` 和 `MAP_PRIVATE` 二者有且仅有其一。
    pub struct MmapFlags: u32 {
        /// 该区域的映射对其他进程可见。若有底层文件，则更新被同步到底层文件上。
        const MAP_SHARED  = 1 << 0;
        /// 私有的 Cow 映射。其他进程不可见，也不会同步到底层文件。
        const MAP_PRIVATE = 1 << 1;

        /// 不只将 `addr` 作为 hint，而是确确实实要求映射在 `addr` 上。
        /// `addr` 必须良好地对齐，大部分情况下是 `PAGE_SIZE` 的整数倍即可。
        /// `addr` 和 `len` 指定一个映射范围，已有的和它重合的映射会被舍弃。
        /// 而如果指定的地址无法被映射，那么 `mmap()` 失败
        const MAP_FIXED     = 1 << 4;
        /// 匿名映射，没有底层文件。内容全部初始化为 0。`fd` 必须为 -1，`offset` 必须为 0。
        const MAP_ANONYMOUS = 1 << 5;
        /// 不为该映射保留 swap 空间。当实际物理内存不足时，可能造成内存溢出。
        const MAP_NORESERVE = 1 << 14;
    }
}
