//! Implementation of [`PageTableEntry`] and [`PageTable`].

use crate::config::PAGE_SIZE;

use super::{frame_alloc, FrameTracker, PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

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
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

/// page table structure
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    pub fn from_token(satp: usize) -> Self {
        const LOW_44_MASK: usize = (1 << 44) - 1;
        // RV64 中 `satp` 低 44 位是根页表的 PPN
        // frames 为空，意味着它只是临时使用，而不管理 frame 的资源
        Self {
            root_ppn: PhysPageNum(satp & LOW_44_MASK),
            frames: Vec::new(),
        }
    }
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.as_page_ptes_mut()[idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&PageTableEntry> = None;
        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &ppn.as_page_ptes_mut()[idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).copied()
    }
    pub fn translate_va_to_pa(&self, va: VirtAddr) -> Option<PhysAddr> {
        self.find_pte(va.floor()).map(|pte| {
            let aligned_pa = pte.ppn().page_start();
            PhysAddr(aligned_pa.0 + va.page_offset())
        })
    }
    pub fn translate_va_as_ref<T>(&self, va: VirtAddr) -> Option<&'static T> {
        self.translate_va_to_pa(va).map(|pa| pa.as_ref())
    }
    pub fn translate_va_as_mut<T>(&self, va: VirtAddr) -> Option<&'static mut T> {
        self.translate_va_to_pa(va).map(|pa| pa.as_mut())
    }
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
    pub fn translate_str(&self, ptr: *const u8) -> Option<String> {
        let mut string = String::new();
        let mut va = ptr as usize;
        // 逐字节地读入字符串，效率较低，但因为字符串可能跨页存在需要如此。
        // NOTE: 可以进一步优化，如一次读一页等
        loop {
            let ch: u8 = *(self.translate_va_as_mut(VirtAddr(va))?);
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
        let mut vpn = start_va.floor();
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

pub fn translated_str(satp: usize, ptr: *const u8) -> Option<String> {
    let page_table = PageTable::from_token(satp);
    page_table.translate_str(ptr)
}

pub fn translated_mut<T>(token: usize, ptr: *mut T) -> &'static mut T {
    let page_table = PageTable::from_token(token);
    page_table
        .translate_va_to_pa(VirtAddr(ptr as usize))
        .unwrap()
        .as_mut()
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
