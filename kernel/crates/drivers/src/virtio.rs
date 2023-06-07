use core::ptr::NonNull;

use super::BlockDevice;
use lazy_static::*;
use memory::{frame_alloc, frame_dealloc, PhysAddr, PhysPageNum};
use utils::{config::PA_TO_VA, upcell::UPSafeCell};
use virtio_drivers::{
    device::blk::VirtIOBlk,
    transport::{
        mmio::{MmioTransport, VirtIOHeader},
        DeviceType, Transport,
    },
    Hal,
};

type BlockDeviceImpl = VirtIOBlock;

lazy_static! {
    pub static ref BLOCK_DEVICE: BlockDeviceImpl = BlockDeviceImpl::new();
}

const VIRTIO0: usize = 0x10001000;

pub struct HalImpl;

unsafe impl Hal for HalImpl {
    fn dma_alloc(
        pages: usize,
        _direction: virtio_drivers::BufferDirection,
    ) -> (virtio_drivers::PhysAddr, NonNull<u8>) {
        let frame = frame_alloc(pages).unwrap();
        // 这个 frame 交由库管理了，要阻止它调用 drop
        let pa_start = frame.ppn.page_start().0;
        core::mem::forget(frame);
        let vptr = NonNull::new((pa_start + PA_TO_VA) as _).unwrap();
        (pa_start, vptr)
    }

    unsafe fn dma_dealloc(
        paddr: virtio_drivers::PhysAddr,
        _vaddr: core::ptr::NonNull<u8>,
        pages: usize,
    ) -> i32 {
        let mut ppn: PhysPageNum = PhysAddr(paddr).ppn();
        frame_dealloc(ppn..PhysPageNum(ppn.0 + pages));
        ppn.0 += 1;
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: virtio_drivers::PhysAddr, _size: usize) -> NonNull<u8> {
        let va = paddr + PA_TO_VA;
        NonNull::new(va as _).unwrap()
    }

    // 不知道 share 和 unshare 干嘛的，先这么实现着
    unsafe fn share(
        buffer: core::ptr::NonNull<[u8]>,
        _direction: virtio_drivers::BufferDirection,
    ) -> virtio_drivers::PhysAddr {
        let vaddr = buffer.addr().get();
        assert!(vaddr >= PA_TO_VA);
        vaddr - PA_TO_VA
    }

    // 在我们的场景中似乎不需要？
    unsafe fn unshare(
        _paddr: virtio_drivers::PhysAddr,
        _buffer: core::ptr::NonNull<[u8]>,
        _direction: virtio_drivers::BufferDirection,
    ) {
    }
}

/// 一个 Wrapper，为了能够满足 `Send` 从而声明为 static
pub struct VirtIOBlock(UPSafeCell<VirtIOBlk<HalImpl, MmioTransport>>);

// NOTE: 暂时不知道这么做行不行，以后再看看
unsafe impl Send for VirtIOBlock {}

impl BlockDevice for VirtIOBlock {
    fn read_block(&self, block_id: u64, buf: &mut [u8]) {
        self.0
            .exclusive_access()
            .read_block(block_id as usize, buf)
            .expect("Error when reading VirtIOBlk");
    }
    fn write_block(&self, block_id: u64, buf: &[u8]) {
        self.0
            .exclusive_access()
            .write_block(block_id as usize, buf)
            .expect("Error when writing VirtIOBlk");
    }
}

impl VirtIOBlock {
    pub fn new() -> Self {
        let header = NonNull::new((VIRTIO0 + PA_TO_VA) as *mut VirtIOHeader).unwrap();
        unsafe {
            let transport = MmioTransport::new(header).unwrap();
            assert!(transport.device_type() == DeviceType::Block);
            VirtIOBlock(UPSafeCell::new(VirtIOBlk::new(transport).unwrap()))
        }
    }
}

impl Default for VirtIOBlock {
    fn default() -> Self {
        Self::new()
    }
}
