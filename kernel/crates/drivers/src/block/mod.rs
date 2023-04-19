mod block_cache;
mod block_device;

pub use block_cache::{block_cache_sync_all, get_block_cache};
pub use block_device::BlockDevice;

pub const BLOCK_SIZE: u32 = 512;
