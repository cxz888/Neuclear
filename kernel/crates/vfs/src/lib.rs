#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug)]
pub enum Error<T> {
    /// 底层文件系统错误
    FsError(T),
    /// 文件类型不正确
    InvalidType,
}

pub trait Fs {
    type FsEntry: Entry;
    fn root_dir(&'static self) -> Self::FsEntry;
}

pub trait Entry {
    type FsError;
    fn is_dir(&self) -> bool;
    fn is_file(&self) -> bool;
    fn ls(&self) -> Result<Vec<String>, Error<Self::FsError>>;
    /// 读取一个文件的所有数据
    fn read_all(&mut self) -> Result<Vec<u8>, Error<Self::FsError>>;
    /// 在当前目录下寻找一个文件或目录
    fn find(&self, name: &str) -> Result<Option<Self>, Error<Self::FsError>>
    where
        Self: Sized;
    fn create(&self, name: &str) -> Result<Self, Error<Self::FsError>>
    where
        Self: Sized;

    fn clear(&mut self) -> bool;
    fn size(&self) -> u64;
}

// pub fn ls(&self) -> Result<Vec<String>, Error<T::FsError>> {
//     self.entry.ls()
// }
// pub fn read_all(&self) -> Result<Vec<u8>, Error<T::FsError>> {
//     self.entry.read_all()
// }
// pub fn find(&self, name: &str) -> Result<Option<Arc<Self>>, Error<T::FsError>> {
//     self.entry.find(name)
// }
// pub fn clear(&self) -> bool {
//     self.entry.clear()
// }
// /// List inodes under current inode
// pub fn ls(&self) -> Vec<String> {
//     // for entry in self.fs
// }
// /// Call a function over a disk inode to read it
// fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
//     get_block_cache(self.block_id, Arc::clone(&self.block_device))
//         .lock()
//         .read(self.block_offset, f)
// }
// /// Call a function over a disk inode to modify it
// fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
//     get_block_cache(self.block_id, Arc::clone(&self.block_device))
//         .lock()
//         .modify(self.block_offset, f)
// }
// /// Find inode under a disk inode by name
// fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
//     // assert it is a directory
//     assert!(disk_inode.is_dir());
//     let file_count = (disk_inode.size as usize) / DIRENT_SZ;
//     let mut dirent = DirEntry::empty();
//     for i in 0..file_count {
//         assert_eq!(
//             disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
//             DIRENT_SZ,
//         );
//         if dirent.name() == name {
//             return Some(dirent.inode_number() as u32);
//         }
//     }
//     None
// }
// /// Find inode under current inode by name
// pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
//     let fs = self.fs.lock();
//     self.read_disk_inode(|disk_inode| {
//         self.find_inode_id(name, disk_inode).map(|inode_id| {
//             let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
//             Arc::new(Self::new(
//                 block_id,
//                 block_offset,
//                 Arc::clone(&self.fs),
//                 Arc::clone(&self.block_device),
//             ))
//         })
//     })
// }
// /// Increase the size of a disk inode
// fn increase_size(
//     &self,
//     new_size: u32,
//     disk_inode: &mut DiskInode,
//     fs: &mut MutexGuard<EasyFileSystem>,
// ) {
//     if new_size < disk_inode.size {
//         return;
//     }
//     let blocks_needed = disk_inode.blocks_num_needed(new_size);
//     let mut v: Vec<u32> = Vec::new();
//     for _ in 0..blocks_needed {
//         v.push(fs.alloc_data());
//     }
//     disk_inode.increase_size(new_size, v, &self.block_device);
// }
// /// Create inode under current inode by name
// pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
//     let mut fs = self.fs.lock();
//     let existed_inode = self.read_disk_inode(|root_inode| {
//         // assert it is a directory
//         assert!(root_inode.is_dir());
//         // has the file been created?
//         self.find_inode_id(name, root_inode)
//     });
//     if existed_inode.is_some() {
//         return None;
//     }
//     // create a new file
//     // alloc a inode with an indirect block
//     let new_inode_id = fs.alloc_inode();
//     // initialize inode
//     let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
//     get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
//         .lock()
//         .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
//             new_inode.initialize(DiskInodeType::File);
//         });
//     self.modify_disk_inode(|root_inode| {
//         // append file in the dirent
//         let file_count = (root_inode.size as usize) / DIRENT_SZ;
//         let new_size = (file_count + 1) * DIRENT_SZ;
//         // increase size
//         self.increase_size(new_size as u32, root_inode, &mut fs);
//         // write dirent
//         let dirent = DirEntry::new(name, new_inode_id);
//         root_inode.write_at(
//             file_count * DIRENT_SZ,
//             dirent.as_bytes(),
//             &self.block_device,
//         );
//     });

//     let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
//     block_cache_sync_all();
//     // return inode
//     Some(Arc::new(Self::new(
//         block_id,
//         block_offset,
//         Arc::clone(&self.fs),
//         Arc::clone(&self.block_device),
//     )))
//     // release efs lock automatically by compiler
// }

// /// Read data from current inode
// pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
//     let _fs = self.fs.lock();
//     self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
// }
// /// Write data to current inode
// pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
//     let mut fs = self.fs.lock();
//     let size = self.modify_disk_inode(|disk_inode| {
//         self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
//         disk_inode.write_at(offset, buf, &self.block_device)
//     });
//     block_cache_sync_all();
//     size
// }
// /// Clear the data in current inode
// pub fn clear(&self) {
//     let mut fs = self.fs.lock();
//     self.modify_disk_inode(|disk_inode| {
//         let size = disk_inode.size;
//         let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
//         assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
//         for data_block in data_blocks_dealloc.into_iter() {
//             fs.dealloc_data(data_block);
//         }
//     });
//     block_cache_sync_all();
// }
