use super::{BlockDevice, BLOCK_SIZE};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

/// Cached block inside memory
pub struct BlockCache {
    /// cached block data
    cache: [u8; BLOCK_SIZE as usize],
    /// underlying block id
    block_id: u64,
    /// underlying block device
    block_device: Arc<dyn BlockDevice>,
    /// whether the block is dirty
    modified: bool,
}

impl BlockCache {
    /// Load a new BlockCache from disk.
    pub fn new(block_id: u64, block_device: Arc<dyn BlockDevice>) -> Self {
        let mut cache = [0u8; BLOCK_SIZE as usize];
        block_device.read_block(block_id, &mut cache);
        Self {
            cache,
            block_id,
            block_device,
            modified: false,
        }
    }
    /// Get the address of an offset inside the cached block data
    fn addr_of_offset(&self, offset: usize) -> usize {
        &self.cache[offset] as *const _ as usize
    }

    pub unsafe fn as_ref<T>(&self, offset: usize) -> &T
    where
        T: Sized,
    {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SIZE as usize);
        let addr = self.addr_of_offset(offset);
        &*(addr as *const T)
    }

    pub unsafe fn as_mut<T>(&mut self, offset: usize) -> &mut T
    where
        T: Sized,
    {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SIZE as usize);
        self.modified = true;
        let addr = self.addr_of_offset(offset);
        &mut *(addr as *mut T)
    }

    pub fn read(&self, offset: u32, buf: &mut [u8]) -> u32 {
        assert!(offset < BLOCK_SIZE);
        let nread = (BLOCK_SIZE - offset).min(buf.len() as u32);
        buf[..nread as usize]
            .copy_from_slice(&self.cache[offset as usize..(offset + nread) as usize]);
        nread
    }

    pub fn write(&mut self, offset: u32, buf: &[u8]) -> u32 {
        assert!(offset < BLOCK_SIZE);
        if buf.len() != 0 {
            self.modified = true;
        }
        let nwrite = (BLOCK_SIZE - offset).min(buf.len() as u32);
        self.cache[offset as usize..(offset + nwrite) as usize]
            .copy_from_slice(&buf[..nwrite as usize]);
        nwrite
    }

    pub fn sync(&mut self) {
        if self.modified {
            self.modified = false;
            self.block_device.write_block(self.block_id, &self.cache);
        }
    }
}

impl Drop for BlockCache {
    fn drop(&mut self) {
        self.sync()
    }
}

/// Use a block cache of 16 blocks
const BLOCK_CACHE_SIZE: usize = 16;

pub struct BlockCacheManager {
    queue: Vec<(u64, Arc<Mutex<BlockCache>>)>,
}

impl BlockCacheManager {
    pub const fn new() -> Self {
        Self { queue: Vec::new() }
    }

    pub fn get_block_cache(
        &mut self,
        block_id: u64,
        block_device: Arc<dyn BlockDevice>,
    ) -> Arc<Mutex<BlockCache>> {
        if let Some(pair) = self.queue.iter().find(|pair| pair.0 == block_id) {
            Arc::clone(&pair.1)
        } else {
            // substitute
            if self.queue.len() == BLOCK_CACHE_SIZE {
                // from front to tail
                if let Some((idx, _)) = self
                    .queue
                    .iter()
                    .enumerate()
                    .find(|(_, pair)| Arc::strong_count(&pair.1) == 1)
                {
                    self.queue.remove(idx);
                } else {
                    panic!("Run out of BlockCache!");
                }
            }
            // load block into mem and push back
            let block_cache = Arc::new(Mutex::new(BlockCache::new(block_id, block_device)));
            self.queue.push((block_id, Arc::clone(&block_cache)));
            block_cache
        }
    }
}

/// The global block cache manager
pub static BLOCK_CACHE_MANAGER: Mutex<BlockCacheManager> = Mutex::new(BlockCacheManager::new());

/// Get the block cache corresponding to the given block id and block device
pub fn get_block_cache(
    block_id: u64,
    block_device: Arc<dyn BlockDevice>,
) -> Arc<Mutex<BlockCache>> {
    BLOCK_CACHE_MANAGER
        .lock()
        .get_block_cache(block_id, block_device)
}

/// Sync all block cache to block device
pub fn block_cache_sync_all() {
    let manager = BLOCK_CACHE_MANAGER.lock();
    for (_, cache) in manager.queue.iter() {
        cache.lock().sync();
    }
}
