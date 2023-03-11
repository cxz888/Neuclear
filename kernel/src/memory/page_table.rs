//! Implementation of [`PageTableEntry`] and [`PageTable`].

use super::{
    frame_alloc, FrameTracker, MapPermission, PhysAddr, PhysPageNum, VirtAddr, VirtPageNum,
};
use crate::config::PAGE_SIZE;
use crate::utils::error::{code, Result};

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

impl From<MapPermission> for PTEFlags {
    fn from(mp: MapPermission) -> Self {
        Self::from_bits_truncate(mp.bits())
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
            // 因为从 root_ppn 开始，只要保证 root_ppn 合法则 pte 合法
            let pte = unsafe { &mut ppn.as_page_ptes_mut()[idx] };
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
    ///
    /// TODO: 是否要将翻译相关的函数返回值改为 Result？
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut ret = None;
        for idx in idxs {
            // 因为从 root_ppn 开始，只要保证 root_ppn 合法则 pte 合法
            let pte = unsafe { &ppn.as_page_ptes()[idx] };
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
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PhysPageNum> {
        self.find_pte(vpn).copied().map(|pte| pte.ppn())
    }
    #[inline]
    pub fn trans_va_to_pa(&self, va: VirtAddr) -> Option<PhysAddr> {
        self.find_pte(va.vpn_floor()).map(|pte| {
            let aligned_pa = pte.ppn().page_start();
            aligned_pa.add(va.page_offset())
        })
    }
    /// 转换用户指针（虚地址）。需要保证该指针指向的是合法的 T，且不会跨越页边界
    #[inline]
    #[track_caller]
    pub unsafe fn trans_ptr<T>(&self, ptr: *const T) -> Result<&'static T> {
        assert!(VirtAddr::from(ptr).page_offset() + core::mem::size_of::<T>() < PAGE_SIZE);
        self.trans_va_to_pa(VirtAddr::from(ptr))
            .map(|pa| pa.as_ref())
            .ok_or(code::EFAULT)
    }
    /// 转换用户指针（虚地址）。需要保证该指针指向的是合法的 T，且不会跨越页边界
    #[inline]
    #[track_caller]
    pub unsafe fn trans_ptr_mut<T>(&mut self, ptr: *mut T) -> Result<&'static mut T> {
        assert!(VirtAddr::from(ptr).page_offset() + core::mem::size_of::<T>() < PAGE_SIZE);
        self.trans_va_to_pa(VirtAddr::from(ptr))
            .map(|pa| pa.as_mut())
            .ok_or(code::EFAULT)
    }
    #[inline]
    pub fn token(&self) -> usize {
        (satp::Mode::Sv39 as usize) << 60 | self.root_ppn.0
    }
    /// 需要保证 `ptr` 指向合法的、空终止的字符串
    pub unsafe fn trans_str(&self, mut ptr: *const u8) -> Result<String> {
        let mut string = Vec::new();
        // 逐字节地读入字符串，效率较低，但因为字符串可能跨页存在需要如此。
        // NOTE: 可以进一步优化，如一次读一页等
        loop {
            let ch: u8 = *(self.trans_ptr(ptr)?);
            if ch == 0 {
                break;
            } else {
                string.push(ch);
                ptr = ptr.add(1);
            }
        }
        String::from_utf8(string).map_err(|_| code::EINVAL)
    }
    /// 最好保证 non-alias。不过一般是用户 buffer
    pub unsafe fn trans_byte_buffer(&mut self, ptr: *mut u8, len: usize) -> Result<Vec<&mut [u8]>> {
        let mut start = VirtAddr::from(ptr);
        let end = start.add(len);
        let mut v = Vec::with_capacity(len / PAGE_SIZE + 2);
        while start < end {
            let mut vpn = start.vpn();
            let mut ppn = self.translate(vpn).ok_or(code::EFAULT)?;
            vpn.0 += 1;
            let mut seg_end = vpn.page_start();
            seg_end = seg_end.min(end);
            if seg_end.page_offset() == 0 {
                v.push(&mut ppn.as_page_bytes_mut()[start.page_offset()..]);
            } else {
                v.push(&mut ppn.as_page_bytes_mut()[start.page_offset()..seg_end.page_offset()]);
            }
            start = seg_end;
        }
        Ok(v)
    }
}

/// An abstraction over a buffer passed from user space to kernel space
#[derive(Debug)]
pub struct UserBuffer<'a> {
    pub buffers: Vec<&'a mut [u8]>,
}

impl<'a> UserBuffer<'a> {
    /// Constuct a UserBuffer
    pub fn new(buffers: Vec<&'a mut [u8]>) -> Self {
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

impl<'a> IntoIterator for UserBuffer<'a> {
    type Item = *mut u8;
    type IntoIter = UserBufferIterator<'a>;
    fn into_iter(self) -> Self::IntoIter {
        UserBufferIterator {
            buffers: self.buffers,
            current_buffer: 0,
            current_idx: 0,
        }
    }
}

// An iterator over a UserBuffer
pub struct UserBufferIterator<'a> {
    buffers: Vec<&'a mut [u8]>,
    current_buffer: usize,
    current_idx: usize,
}

impl Iterator for UserBufferIterator<'_> {
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
