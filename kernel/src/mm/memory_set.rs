//! Implementation of [`MapArea`] and [`MemorySet`].

use super::{
    frame_alloc, FrameTracker, PTEFlags, PageTable, PageTableEntry, PhysAddr, PhysPageNum,
    VirtAddr, VirtPageNum,
};
use crate::{
    config::{MEMORY_END, MMIO, PAGE_SIZE, PAGE_SIZE_BITS, SECOND_START, TRAMPOLINE},
    error::{code, Result},
    sync::UPSafeCell,
    syscall::MmapProt,
};

use alloc::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    vec::Vec,
};
use bitflags::bitflags;
use core::ops::Range;
use goblin::elf::{
    header::ET_EXEC,
    program_header,
    program_header::{PF_R, PF_W, PF_X, PT_LOAD},
    Elf,
};
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
    page_table: PageTable,
    areas: BTreeSet<MapArea>,
}

impl MemorySet {
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: BTreeSet::new(),
        }
    }
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
    /// 尝试在 `vm_range` 进行映射
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
            let mut start = VirtPageNum((SECOND_START + PAGE_SIZE) >> PAGE_SIZE_BITS);
            let len = vpn_range.end.0 - vpn_range.start.0;
            for area in &self.areas {
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
    /// Assume that no conflicts.
    pub fn insert_framed_area(
        &mut self,
        start_vpn: VirtPageNum,
        end_vpn: VirtPageNum,
        perm: MapPermission,
    ) {
        self.push(
            MapArea {
                vpn_range: start_vpn..end_vpn,
                map_type: MapType::Framed {
                    data_frames: BTreeMap::new(),
                },
                map_perm: perm,
            },
            0,
            None,
        );
    }
    pub fn remove_area_with_start_vpn(&mut self, start_vpn: VirtPageNum) {
        if let Some(mut area) = self.areas.take(&MapArea::with_start_vpn(start_vpn)) {
            area.unmap(&mut self.page_table);
        }
    }
    fn push(&mut self, mut map_area: MapArea, start_offset: usize, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, start_offset, data);
        }
        self.areas.insert(map_area);
    }
    /// Mention that trampoline is not collected by areas.
    fn map_trampoline(&mut self) {
        self.page_table.map(
            VirtAddr(TRAMPOLINE).vpn(),
            PhysAddr(strampoline as usize).ppn(),
            PTEFlags::R | PTEFlags::X,
        );
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
        // log::info!("mapping .bss section");
        // memory_set.push(
        //     MapArea::new(
        //         VirtAddr(sbss_with_stack as usize),
        //         VirtAddr(ebss as usize),
        //         MapType::Identical,
        //         MapPermission::R | MapPermission::W,
        //     ),
        //     0,
        //     None,
        // );
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
    /// 根据 ELF 文件内容加载所有 section，并且映射 trampoline、TrapContext 和用户栈，
    /// 同时也会返回 `brk` 的初始值 和 `entry_point`
    ///
    /// ELF 标准参考 <https://www.sco.com/developers/gabi/latest/ch5.pheader.html>
    /// 和 <https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-elf.adoc>
    pub fn from_elf(elf: &Elf, elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();
        memory_set.map_trampoline();

        // 映射 ELF 中所有段
        assert!(elf.is_64);
        log::debug!("e_flags: {:#b}", elf.header.e_flags);
        assert_eq!(elf.header.e_type, ET_EXEC);
        // 确定是 RISC-V 执行环境
        assert_eq!(elf.header.e_machine, 243);
        log::debug!("e_phoff: {}", elf.header.e_phoff);

        let mut elf_base = 0;

        // 加载段
        let mut max_end_vpn = VirtPageNum(0);
        for ph in &elf.program_headers {
            log::debug!("ph_type: {:?}", program_header::pt_to_str(ph.p_type));
            log::debug!("ph range: {:#x?}", ph.vm_range());
            if ph.p_type == PT_LOAD {
                // 记录一下 elf 的起始地址
                if ph.p_offset == 0 {
                    elf_base = ph.p_vaddr;
                }
                let start_va = VirtAddr(ph.p_vaddr as usize);
                let start_offset = start_va.page_offset();
                let end_va = VirtAddr((ph.p_vaddr + ph.p_memsz) as usize);
                let mut map_perm = MapPermission::U;
                if ph.p_flags & PF_R != 0 {
                    map_perm |= MapPermission::R;
                }
                if ph.p_flags & PF_W != 0 {
                    map_perm |= MapPermission::W;
                }
                if ph.p_flags & PF_X != 0 {
                    map_perm |= MapPermission::X;
                }
                let map_area = MapArea::new(
                    start_va,
                    end_va,
                    MapType::Framed {
                        data_frames: BTreeMap::new(),
                    },
                    map_perm,
                );
                max_end_vpn = map_area.vpn_range.end;
                memory_set.push(map_area, start_offset, Some(&elf_data[ph.file_range()]));
                log::debug!("map_perm: {:?}", map_perm);
            }
        }
        log::info!("entry point: {:#x}", elf.entry);
        // We don't map user stack and trapframe here since they will be later
        // allocated through TaskControlBlock::new()
        let program_break = max_end_vpn.page_start().0;
        (memory_set, program_break, elf.entry as usize)
    }
    /// Copy an identical user_space
    pub fn from_existed_user(user_space: &MemorySet) -> MemorySet {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // copy data sections/trap_context/user_stack
        for area in user_space.areas.iter() {
            let new_area = MapArea::from_another(area);
            memory_set.push(new_area, 0, None);
            // copy data from another space
            for vpn in area.vpn_range.clone() {
                let src_ppn = user_space.translate(vpn).unwrap().ppn();
                let mut dst_ppn = memory_set.translate(vpn).unwrap().ppn();
                dst_ppn
                    .as_page_bytes_mut()
                    .copy_from_slice(src_ppn.as_page_bytes());
            }
        }
        memory_set
    }
    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            core::arch::asm!("sfence.vma");
        }
    }
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
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
    pub fn framed() -> Self {
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

impl PartialEq for MapArea {
    fn eq(&self, other: &Self) -> bool {
        self.vpn_range.start == other.vpn_range.start
    }
}

impl Eq for MapArea {}

impl PartialOrd for MapArea {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.vpn_range.start.partial_cmp(&other.vpn_range.start)
    }
}

impl Ord for MapArea {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl MapArea {
    pub fn with_start_vpn(start_vpn: VirtPageNum) -> Self {
        Self {
            vpn_range: start_vpn..start_vpn,
            map_type: MapType::Identical,
            map_perm: MapPermission::empty(),
        }
    }
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
    /// 约定：当前逻辑段必须是 `Framed` 的。而且 `data` 的长度不得超过逻辑段长度。
    pub fn copy_data(&mut self, page_table: &mut PageTable, start_offset: usize, data: &[u8]) {
        assert!(matches!(self.map_type, MapType::Framed { .. }));
        assert!(start_offset < PAGE_SIZE);
        let mut curr_vpn = self.vpn_range.start;

        let (first_block, rest) = data.split_at((PAGE_SIZE - start_offset).min(data.len()));

        page_table
            .translate(curr_vpn)
            .unwrap()
            .ppn()
            .copy_from(start_offset, first_block);
        curr_vpn.0 += 1;

        for chunk in rest.chunks(PAGE_SIZE) {
            let mut dst = page_table.translate(curr_vpn).unwrap().ppn();
            dst.copy_from(0, chunk);
            curr_vpn.0 += 1;
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

impl Into<PTEFlags> for MapPermission {
    fn into(self) -> PTEFlags {
        PTEFlags::from_bits_truncate(self.bits)
    }
}
