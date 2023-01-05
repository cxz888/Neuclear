#![no_std]
#![feature(mixed_integer_ops)]
#![feature(generic_associated_types)]

extern crate alloc;

use alloc::{borrow::Cow, string::String, sync::Arc, vec::Vec};
use drivers::{block_cache_sync_all, get_block_cache, BlockDevice, BLOCK_SIZE};
use fatfs::{
    Dir, DirEntry, Error, File, FileSystem, FsOptions, IoBase, LossyOemCpConverter,
    NullTimeProvider, Read, Seek, SeekFrom, Write,
};
use vfs::{Entry, Fs};

// TODO: 把 Fat32 的内部实现改为 Send + Sync
unsafe impl Send for Fat32 {}
unsafe impl Sync for Fat32 {}

pub struct Fat32 {
    fs: FileSystem<DiskDriver, NullTimeProvider, LossyOemCpConverter>,
}

impl Fat32 {
    pub fn new(device: Arc<dyn BlockDevice>) -> Self {
        let fs = FileSystem::new(
            DiskDriver {
                device,
                block_id: 0,
                block_offset: 0,
            },
            FsOptions::new(),
        )
        .unwrap();

        Self { fs }
    }
}

// TODO: 把 Fat32 的内部实现改为 Send + Sync
unsafe impl Send for Fat32Entry {}
unsafe impl Sync for Fat32Entry {}

impl Fs for Fat32 {
    type FsEntry = Fat32Entry;
    fn root_dir(&'static self) -> Self::FsEntry {
        Fat32Entry::Dir(self.fs.root_dir())
    }
}
pub enum Fat32Entry {
    Dir(Dir<'static, DiskDriver, NullTimeProvider, LossyOemCpConverter>),
    File(File<'static, DiskDriver, NullTimeProvider, LossyOemCpConverter>),
}

type VfsError = vfs::Error<Error<()>>;

impl Entry for Fat32Entry {
    type FsError = Error<()>;
    #[inline]
    fn is_dir(&self) -> bool {
        matches!(self, Fat32Entry::Dir(_))
    }
    #[inline]
    fn is_file(&self) -> bool {
        matches!(self, Fat32Entry::File(_))
    }
    fn ls(&self) -> Result<Vec<String>, VfsError> {
        let dir = match self {
            Fat32Entry::Dir(dir) => dir,
            Fat32Entry::File(entry) => {
                return Err(VfsError::InvalidType);
            }
        };

        let mut files = Vec::new();
        for entry in dir.iter() {
            let entry = entry.map_err(|e| VfsError::FsError(e))?;
            files.push(entry.file_name())
        }
        Ok(files)
    }

    fn read_all(&mut self) -> Result<Vec<u8>, VfsError> {
        let file = match self {
            Fat32Entry::Dir(_) => return Err(VfsError::InvalidType),
            Fat32Entry::File(file) => file,
        };
        // TODO: 想办法获取文件长度
        let mut ret = Vec::new();
        let mut buf = [0; BLOCK_SIZE as usize];
        let mut nread = usize::MAX;
        while nread != 0 {
            nread = file.read(&mut buf).map_err(|e| VfsError::FsError(e))?;
            ret.extend_from_slice(&buf[..nread])
        }
        // assert_eq!(ret.len(), size);
        Ok(ret)
    }
    fn find(&self, name: &str) -> Result<Option<Self>, VfsError> {
        let dir = match self {
            Fat32Entry::Dir(dir) => dir,
            Fat32Entry::File(_) => {
                return Err(VfsError::InvalidType);
            }
        };
        for entry in dir.iter() {
            let entry = entry.map_err(|e| VfsError::FsError(e))?;
            if entry.file_name() == name {
                if entry.is_dir() {
                    return Ok(Some(Fat32Entry::Dir(entry.to_dir())));
                } else {
                    return Ok(Some(Fat32Entry::File(entry.to_file())));
                }
            }
        }
        return Ok(None);
    }
    /// 创建文件，若已存在则只是打开
    fn create(&self, name: &str) -> Result<Self, VfsError> {
        let dir = match self {
            Fat32Entry::Dir(root) => root,
            Fat32Entry::File(_) => {
                return Err(VfsError::InvalidType);
            }
        };
        match dir.create_file(name) {
            Ok(file) => Ok(Fat32Entry::File(file)),
            Err(e) => Err(VfsError::FsError(e)),
        }
    }
    fn clear(&mut self) -> bool {
        match self {
            Fat32Entry::Dir(_) => return false,
            Fat32Entry::File(file) => {
                file.seek(SeekFrom::Start(0));
                file.truncate().is_err()
            }
        }
    }
}

/// 一个利用缓存的磁盘驱动器。
///
/// NOTE: 由于目前没有什么好的手段确定磁盘的总大小，假定读入时不会超过总大小
pub struct DiskDriver {
    device: Arc<dyn BlockDevice>,
    block_id: u64,
    block_offset: u32,
}
impl IoBase for DiskDriver {
    type Error = ();
}
impl Read for DiskDriver {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // 不断地从缓存中读入数据。如果 buf 很长，就持续访问下一 block
        let mut tot_nread = 0;
        let tot = buf.len();
        while tot_nread < tot {
            let nread = get_block_cache(self.block_id, Arc::clone(&self.device))
                .lock()
                .read(self.block_offset, &mut buf[tot_nread..]);
            self.block_offset += nread;
            if self.block_offset >= BLOCK_SIZE {
                self.block_id += 1;
                self.block_offset -= BLOCK_SIZE;
            }
            tot_nread += nread as usize;
        }
        Ok(tot_nread)
    }
}
impl Write for DiskDriver {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        // 不断地向缓存中写入数据。如果 buf 很长，就持续访问下一 block
        let mut tot_nwrite = 0;
        let tot = buf.len();
        while tot_nwrite < tot {
            let nwrite = get_block_cache(self.block_id, Arc::clone(&self.device))
                .lock()
                .write(self.block_offset, &buf[tot_nwrite..]);
            self.block_offset += nwrite;
            if self.block_offset >= BLOCK_SIZE {
                self.block_id += 1;
                self.block_offset -= BLOCK_SIZE;
            }
            tot_nwrite += nwrite as usize;
        }
        Ok(tot_nwrite)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        block_cache_sync_all();
        Ok(())
    }
}
impl Seek for DiskDriver {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let offset = match pos {
            fatfs::SeekFrom::Start(from_start) => from_start,
            fatfs::SeekFrom::End(_) => todo!("目前无法得知设备总大小"),
            fatfs::SeekFrom::Current(from_curr) => (self.block_id * BLOCK_SIZE as u64
                + self.block_offset as u64)
                .checked_add_signed(from_curr)
                .ok_or(())?,
        };
        self.block_id = offset / BLOCK_SIZE as u64;
        self.block_offset = (offset % BLOCK_SIZE as u64) as u32;
        Ok(offset)
    }
}
