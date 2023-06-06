#![no_std]
#![feature(strict_provenance)]

extern crate alloc;

mod block;
mod virtio;

pub use block::{block_cache_sync_all, get_block_cache, BlockDevice, BLOCK_SIZE};
pub use virtio::BLOCK_DEVICE;
