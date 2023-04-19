use super::BlockDevice;
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::*;
use memory::{
    frame_alloc, frame_dealloc, kernel_pa_to_va, kernel_va_to_pa, FrameTracker, PhysAddr,
    PhysPageNum, VirtAddr,
};
use utils::{config::PA_TO_VA, upcell::UPSafeCell};
use virtio_drivers::{VirtIOBlk, VirtIOHeader};

type BlockDeviceImpl = VirtIOBlock;

lazy_static! {
    pub static ref BLOCK_DEVICE: Arc<dyn BlockDevice> = Arc::new(BlockDeviceImpl::new());
}

const VIRTIO0: usize = 0x10001000;

pub struct VirtIOBlock(UPSafeCell<VirtIOBlk<'static>>);

lazy_static! {
    static ref QUEUE_FRAMES: UPSafeCell<Vec<FrameTracker>> = unsafe { UPSafeCell::new(Vec::new()) };
}

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
        unsafe {
            Self(UPSafeCell::new(
                VirtIOBlk::new(&mut *((VIRTIO0 + PA_TO_VA) as *mut VirtIOHeader)).unwrap(),
            ))
        }
    }
}

#[no_mangle]
pub extern "C" fn virtio_dma_alloc(pages: usize) -> PhysAddr {
    let mut ppn_base = PhysPageNum(0);
    for i in 0..pages {
        let frame = frame_alloc().unwrap();
        if i == 0 {
            ppn_base = frame.ppn;
        }
        assert_eq!(frame.ppn.0, ppn_base.0 + i);
        QUEUE_FRAMES.exclusive_access().push(frame);
    }
    ppn_base.page_start()
}

#[no_mangle]
pub extern "C" fn virtio_dma_dealloc(pa: PhysAddr, pages: usize) -> i32 {
    let mut ppn_base: PhysPageNum = pa.ppn();
    for _ in 0..pages {
        frame_dealloc(ppn_base);
        ppn_base.0 += 1;
    }
    0
}

#[no_mangle]
pub extern "C" fn virtio_phys_to_virt(paddr: PhysAddr) -> VirtAddr {
    kernel_pa_to_va(paddr)
}

#[no_mangle]
pub extern "C" fn virtio_virt_to_phys(vaddr: VirtAddr) -> PhysAddr {
    kernel_va_to_pa(vaddr)
}
