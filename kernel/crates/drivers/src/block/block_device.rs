/// Trait for block devices
/// which reads and writes data in the unit of blocks
pub trait BlockDevice: Send + Sync {
    fn read_block(&self, block_id: u64, buf: &mut [u8]);
    fn write_block(&self, block_id: u64, buf: &[u8]);
}
