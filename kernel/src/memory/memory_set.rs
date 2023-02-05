//! Implementation of [`MapArea`] and [`MemorySet`].

use super::{
    frame_alloc, FrameTracker, PTEFlags, PageTable, PhysAddr, PhysPageNum, VirtAddr, VirtPageNum,
};
use crate::{
    config::{MEMORY_END, MMIO, PAGE_SIZE, PAGE_SIZE_BITS, SECOND_START, TRAMPOLINE},
    sync::UPSafeCell,
    syscall::MmapProt,
    utils::error::{code, Result},
};

use alloc::{collections::BTreeMap, sync::Arc};
use bitflags::bitflags;
use core::{assert_matches::assert_matches, ops::Range};
use lazy_static::*;
use riscv::register::satp;

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

lazy_static! {
    /// a memory set instance through lazy_static! managing kernel space
    pub static ref KERNEL_SPACE: UPSafeCell<MemorySet> =unsafe {
        UPSafeCell::new(MemorySet::new_kernel())
    };
}

/// Get the token of the kernel memory space
pub fn kernel_token() -> usize {
    KERNEL_SPACE.exclusive_access().token()
}

/// memory set structure, controls virtual-memory space
pub struct MemorySet {
    pub page_table: PageTable,
    // 起始 vpn 映射到 MapArea
    areas: BTreeMap<VirtPageNum, MapArea>,
}

impl MemorySet {
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: BTreeMap::new(),
        }
    }

    /// Without kernel stacks.
    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // map kernel sections
        log::info!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        log::info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        log::info!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        log::info!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize,
            ebss as usize
        );
        log::info!("mapping .text section");
        memory_set.push(
            MapArea::new(
                VirtAddr(stext as usize),
                VirtAddr(etext as usize),
                MapType::Identical,
                MapPermission::R | MapPermission::X,
            ),
            0,
            None,
        );
        log::info!("mapping .rodata section");
        memory_set.push(
            MapArea::new(
                VirtAddr(srodata as usize),
                VirtAddr(erodata as usize),
                MapType::Identical,
                MapPermission::R,
            ),
            0,
            None,
        );
        // .data 段和 .bss 段的访问限制相同，所以可以放到一起
        log::info!("mapping .data and .bss section");
        memory_set.push(
            MapArea::new(
                VirtAddr(sdata as usize),
                VirtAddr(ebss as usize),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            0,
            None,
        );
        log::info!("mapping physical memory");
        memory_set.push(
            MapArea::new(
                VirtAddr(ekernel as usize),
                VirtAddr(MEMORY_END),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            0,
            None,
        );
        log::info!("mapping memory-mapped registers");
        for &(start, len) in MMIO {
            memory_set.push(
                MapArea::new(
                    VirtAddr(start),
                    VirtAddr(start + len),
                    MapType::Identical,
                    MapPermission::R | MapPermission::W,
                ),
                0,
                None,
            );
        }
        memory_set
    }

    /// Copy an identical user_space
    pub fn from_existed_user(user_space: &MemorySet) -> MemorySet {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // copy data sections/trap_context/user_stack
        for (_, area) in user_space.areas.iter() {
            let new_area = MapArea::from_another(area);
            memory_set.push(new_area, 0, None);
            // copy data from another space
            for vpn in area.vpn_range.clone() {
                let src_ppn = user_space.translate(vpn).unwrap();
                let mut dst_ppn = memory_set.translate(vpn).unwrap();
                unsafe {
                    dst_ppn
                        .as_page_bytes_mut()
                        .copy_from_slice(src_ppn.as_page_bytes());
                }
            }
        }
        memory_set
    }

    pub fn token(&self) -> usize {
        self.page_table.token()
    }

    /// 需保证 heap_start < new_vpn，且还有足够的虚地址和物理空间可以映射
    pub fn set_user_brk(&mut self, new_end: VirtPageNum, heap_start: VirtPageNum) {
        // 堆区已经映射过了，就扩张或者收缩。否则插入堆区
        if let Some(map_area) = self.areas.get_mut(&heap_start) {
            let curr_vpn = map_area.end();
            if curr_vpn >= new_end {
                map_area.shrink(new_end, &mut self.page_table);
            } else {
                map_area.expand(new_end, &mut self.page_table);
            }
        } else {
            self.insert_framed_area(
                heap_start,
                new_end,
                MapPermission::R | MapPermission::W | MapPermission::U,
            );
        }
    }

    /// 尝试根据 `vm_range` 进行映射
    pub fn try_map(
        &mut self,
        vpn_range: Range<VirtPageNum>,
        perm: MapPermission,
        fixed: bool,
    ) -> Result<isize> {
        // TODO: 这里，如果是位置固定的映射，那么应当 unmap 与其相交的部分
        log::debug!("perm: {perm:?}");
        if fixed {
            log::error!("should unmap intersecting part");
            self.insert_framed_area(vpn_range.start, vpn_range.end, perm);
            Ok(vpn_range.start.0 as isize)
        } else {
            // 尝试在高地址空间找到一个合适的段来映射
            let mut start = VirtPageNum((SECOND_START) >> PAGE_SIZE_BITS);
            let len = vpn_range.end.0 - vpn_range.start.0;
            for area in self.areas.values() {
                // 高地址空间，但同时要控制住不溢出
                if area.vpn_range.start > start && start < VirtPageNum(TRAMPOLINE >> PAGE_SIZE_BITS)
                {
                    if start.add(len) <= area.vpn_range.start {
                        // TODO: 匿名映射的话，按照约定应当全部初始化为 0
                        self.insert_framed_area(start, start.add(len), perm);
                        return Ok(start.page_start().0 as isize);
                    }
                    start = area.vpn_range.end
                }
            }
            Err(code::ENOMEM)
        }
    }

    /// 插入帧映射的一个内存段，假定是不会造成冲突的
    pub fn insert_framed_area(
        &mut self,
        start_vpn: VirtPageNum,
        end_vpn: VirtPageNum,
        perm: MapPermission,
    ) {
        self.push(
            MapArea {
                vpn_range: start_vpn..end_vpn,
                map_type: MapType::new_framed(),
                map_perm: perm,
            },
            0,
            None,
        );
    }

    pub fn remove_area_with_start_vpn(&mut self, start_vpn: VirtPageNum) {
        if let Some(mut area) = self.areas.remove(&start_vpn) {
            area.unmap(&mut self.page_table);
        }
    }

    pub fn push(&mut self, mut map_area: MapArea, start_offset: usize, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, start_offset, data);
        }
        self.areas.insert(map_area.vpn_range.start, map_area);
    }
    /// trampoline 是不由 areas 收集的，因为它是在所有进程之间和内核之间共享的
    pub fn map_trampoline(&mut self) {
        self.page_table.map(
            VirtAddr(TRAMPOLINE).vpn(),
            PhysAddr(strampoline as usize).ppn(),
            PTEFlags::R | PTEFlags::X,
        );
    }

    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            core::arch::asm!("sfence.vma");
        }
    }
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PhysPageNum> {
        self.page_table.translate(vpn)
    }
    pub fn recycle_data_pages(&mut self) {
        //*self = Self::new_bare();
        self.areas.clear();
    }
}

/// 描述逻辑段内所有虚拟页映射到物理页的方式
#[derive(Debug, Clone)]
pub enum MapType {
    /// 恒等映射，或者说直接以物理地址访问
    Identical,
    /// 需要分配物理页帧
    Framed {
        /// 这些保存的物理页帧用于存放实际的内存数据
        ///
        /// 而 PageTable 所拥有的的物理页仅用于存放页表节点数据，因此不会冲突
        data_frames: BTreeMap<VirtPageNum, Arc<FrameTracker>>,
    },
}

impl MapType {
    /// 空的帧映射
    pub fn new_framed() -> Self {
        Self::Framed {
            data_frames: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MapArea {
    pub vpn_range: Range<VirtPageNum>,
    map_type: MapType,
    map_perm: MapPermission,
}

impl MapArea {
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.vpn_floor();
        let end_vpn: VirtPageNum = end_va.vpn_ceil();
        Self {
            vpn_range: start_vpn..end_vpn,
            map_type,
            map_perm,
        }
    }
    pub fn from_another(another: &MapArea) -> Self {
        Self {
            vpn_range: another.vpn_range.clone(),
            map_type: another.map_type.clone(),
            map_perm: another.map_perm,
        }
    }
    pub fn len(&self) -> usize {
        self.vpn_range.end.0.saturating_sub(self.vpn_range.start.0) * PAGE_SIZE
    }
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn;
        match &mut self.map_type {
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            MapType::Framed { data_frames } => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                data_frames.insert(vpn, Arc::new(frame));
            }
        }
        page_table.map(vpn, ppn, PTEFlags::from_bits_truncate(self.map_perm.bits));
    }

    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        if let MapType::Framed { data_frames } = &mut self.map_type {
            data_frames.remove(&vpn);
        }
        page_table.unmap(vpn);
    }
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range.clone() {
            self.map_one(page_table, vpn);
        }
    }
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range.clone() {
            self.unmap_one(page_table, vpn);
        }
    }
    #[inline]
    pub fn end(&self) -> VirtPageNum {
        self.vpn_range.end
    }
    /// 尝试收缩末尾区域
    pub fn shrink(&mut self, new_end: VirtPageNum, page_table: &mut PageTable) {
        for vpn in new_end..self.end() {
            self.unmap_one(page_table, vpn);
        }
        self.vpn_range.end = new_end;
    }
    /// 尝试扩展末尾区域
    pub fn expand(&mut self, new_end: VirtPageNum, page_table: &mut PageTable) {
        for vpn in self.end()..new_end {
            self.map_one(page_table, vpn);
        }
        self.vpn_range.end = new_end;
    }
    /// 约定：当前逻辑段必须是 `Framed` 的。而且 `data` 的长度不得超过逻辑段长度。
    pub fn copy_data(&mut self, page_table: &mut PageTable, start_offset: usize, data: &[u8]) {
        assert_matches!(self.map_type, MapType::Framed { .. });
        assert!(start_offset < PAGE_SIZE);
        assert!(data.len() <= self.len());
        let mut curr_vpn = self.vpn_range.start;

        let (first_block, rest) = data.split_at((PAGE_SIZE - start_offset).min(data.len()));
        log::debug!("first_block: {:#x}", first_block.len());
        log::debug!("rest: {:#x}", rest.len());

        unsafe {
            page_table
                .translate(curr_vpn)
                .unwrap()
                .copy_from(start_offset, first_block);

            curr_vpn.0 += 1;

            for chunk in rest.chunks(PAGE_SIZE) {
                let mut dst = page_table.translate(curr_vpn).unwrap();
                dst.copy_from(0, chunk);
                curr_vpn.0 += 1;
            }
        }
    }
}

bitflags! {
    /// map permission corresponding to that in pte: `R W X U`
    pub struct MapPermission: u8 {
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
    }
}

impl From<MmapProt> for MapPermission {
    fn from(mmap_prot: MmapProt) -> Self {
        Self::from_bits_truncate((mmap_prot.bits() << 1) as u8) | MapPermission::U
    }
}
