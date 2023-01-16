//! Implementation of [`PageTableEntry`] and [`PageTable`].

use crate::config::PAGE_SIZE;
use crate::syscall::MmapProt;

use super::{
    frame_alloc, FrameTracker, MapPermission, PhysAddr, PhysPageNum, VirtAddr, VirtPageNum,
};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;
use riscv::register::satp;

bitflags! {
    /// page table entry flags
    pub struct PTEFlags: u8 {
        const V = 1 << 0;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
        const G = 1 << 5;
        const A = 1 << 6;
        const D = 1 << 7;
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
/// page table entry structure
pub struct PageTableEntry {
    pub bits: usize,
}

impl PageTableEntry {
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    pub fn ppn(&self) -> PhysPageNum {
        const LOW_44_MASK: usize = (1 << 44) - 1;
        PhysPageNum((self.bits >> 10) & LOW_44_MASK)
    }
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits_truncate(self.bits as u8)
    }
    pub fn is_valid(&self) -> bool {
        self.flags().contains(PTEFlags::V)
    }
}

/// page table structure
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>,
}

/// 假定创建和映射时不会导致内存不足
impl PageTable {
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }
    /// 从 token 生成临时页表。用于在内核态根据用户提供的虚地址访问用户数据。
    pub fn from_token(token: usize) -> Self {
        const LOW_44_MASK: usize = (1 << 44) - 1;
        // RV64 中 `satp` 低 44 位是根页表的 PPN
        // frames 为空，意味着它只是临时使用，而不管理 frame 的资源
        Self {
            root_ppn: PhysPageNum(token & LOW_44_MASK),
            frames: Vec::new(),
        }
    }
    /// 找到 `vpn` 对应的叶子页表项。注意不保证该页表项 valid，需调用方自己修改
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut ret: Option<&mut PageTableEntry> = None;
        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.as_page_ptes_mut()[idx];
            // 这里假定为 3 级页表
            if i == 2 {
                ret = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        ret
    }
    /// 找到 `vpn` 对应的叶子页表项。注意，该页表项必须是 valid 的。
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut ret = None;
        for idx in idxs {
            let pte = &ppn.as_page_ptes()[idx];
            if !pte.is_valid() {
                return None;
            }
            ret = Some(pte);
            ppn = pte.ppn();
        }
        ret
    }
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(
            !pte.is_valid(),
            "vpn {:#x?} is mapped before mapping",
            vpn.0
        );
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }
    #[inline]
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).copied()
    }
    pub fn translate_va_to_pa(&self, va: VirtAddr) -> Option<PhysAddr> {
        self.find_pte(va.vpn_floor()).map(|pte| {
            let aligned_pa = pte.ppn().page_start();
            aligned_pa.add(va.page_offset())
        })
    }
    #[inline]
    pub fn trans_va_as_ref<T>(&self, va: VirtAddr) -> Option<&'static T> {
        self.translate_va_to_pa(va).map(|pa| pa.as_ref())
    }
    #[inline]
    pub fn trans_va_as_mut<T>(&self, va: VirtAddr) -> Option<&'static mut T> {
        self.translate_va_to_pa(va).map(|pa| pa.as_mut())
    }
    #[inline]
    pub fn token(&self) -> usize {
        (satp::Mode::Sv39 as usize) << 60 | self.root_ppn.0
    }
    pub fn translate_str(&self, ptr: *const u8) -> Option<String> {
        let mut string = String::new();
        let mut va = ptr as usize;
        // 逐字节地读入字符串，效率较低，但因为字符串可能跨页存在需要如此。
        // NOTE: 可以进一步优化，如一次读一页等
        loop {
            let ch: u8 = *(self.trans_va_as_mut(VirtAddr(va))?);
            if ch == 0 {
                break;
            } else {
                string.push(ch as char);
                va += 1;
            }
        }
        Some(string)
    }
}

/// translate a pointer to a mutable u8 Vec through page table
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::with_capacity(len / PAGE_SIZE + 2);
    while start < end {
        let start_va = VirtAddr(start);
        let mut vpn = start_va.vpn_floor();
        let mut ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.0 += 1;
        let mut end_va = vpn.page_start();
        end_va = end_va.min(VirtAddr(end));
        if end_va.page_offset() == 0 {
            v.push(&mut ppn.as_page_bytes_mut()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.as_page_bytes_mut()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.0;
    }
    v
}

/// An abstraction over a buffer passed from user space to kernel space
pub struct UserBuffer {
    pub buffers: Vec<&'static mut [u8]>,
}

impl UserBuffer {
    /// Constuct a UserBuffer
    pub fn new(buffers: Vec<&'static mut [u8]>) -> Self {
        Self { buffers }
    }
    /// Get the length of a UserBuffer
    pub fn len(&self) -> usize {
        let mut total: usize = 0;
        for b in self.buffers.iter() {
            total += b.len();
        }
        total
    }
}

impl IntoIterator for UserBuffer {
    type Item = *mut u8;
    type IntoIter = UserBufferIterator;
    fn into_iter(self) -> Self::IntoIter {
        UserBufferIterator {
            buffers: self.buffers,
            current_buffer: 0,
            current_idx: 0,
        }
    }
}

// An iterator over a UserBuffer
pub struct UserBufferIterator {
    buffers: Vec<&'static mut [u8]>,
    current_buffer: usize,
    current_idx: usize,
}

impl Iterator for UserBufferIterator {
    type Item = *mut u8;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_buffer >= self.buffers.len() {
            None
        } else {
            let r = &mut self.buffers[self.current_buffer][self.current_idx] as *mut _;
            if self.current_idx + 1 == self.buffers[self.current_buffer].len() {
                self.current_idx = 0;
                self.current_buffer += 1;
            } else {
                self.current_idx += 1;
            }
            Some(r)
        }
    }
}
