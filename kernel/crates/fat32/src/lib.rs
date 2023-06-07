#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use drivers::{block_cache_sync_all, get_block_cache, BlockDevice, BLOCK_SIZE};
use fatfs::{
    Dir, Error, File, FileSystem, FsOptions, IoBase, LossyOemCpConverter, NullTimeProvider, Read,
    Seek, SeekFrom, Write,
};
use vfs::{Entry, Fs};

// TODO: 把 Fat32 的内部实现改为 Send + Sync
unsafe impl Send for Fat32 {}
unsafe impl Sync for Fat32 {}

pub struct Fat32 {
    fs: FileSystem<DiskDriver, NullTimeProvider, LossyOemCpConverter>,
}

impl Fat32 {
    pub fn new(device: &'static dyn BlockDevice) -> Self {
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
            Fat32Entry::File(_) => {
                return Err(VfsError::InvalidType);
            }
        };

        let mut files = Vec::new();
        for entry in dir.iter() {
            let entry = entry.map_err(VfsError::FsError)?;
            files.push(entry.file_name())
        }
        Ok(files)
    }
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VfsError> {
        let Fat32Entry::File(file) = self else {
            return Err(VfsError::InvalidType);
        };
        file.read(buf).map_err(VfsError::FsError)
    }
    fn read_all(&mut self) -> Result<Vec<u8>, VfsError> {
        let Fat32Entry::File(file) = self else {
            return Err(VfsError::InvalidType);
        };
        // TODO: 想办法获取文件长度
        let mut ret = Vec::new();
        let mut buf = [0; BLOCK_SIZE as usize];
        let mut nread = usize::MAX;
        while nread != 0 {
            nread = file.read(&mut buf).map_err(VfsError::FsError)?;
            ret.extend_from_slice(&buf[..nread])
        }
        // assert_eq!(ret.len(), size);
        Ok(ret)
    }
    fn write(&mut self, buf: &[u8]) -> Result<usize, VfsError> {
        let Fat32Entry::File(file) = self else {
            return Err(VfsError::InvalidType);
        };
        file.write(buf).map_err(VfsError::FsError)
    }
    fn remove(&self, name: &str) -> Result<(), VfsError> {
        let Fat32Entry::Dir(dir) = self else {
            return Err(VfsError::InvalidType);
        };
        dir.remove(name).map_err(VfsError::FsError)
    }
    /// 在目录下寻找一个条目。
    ///
    /// 若当前节点不是目录，或者寻找过程中发生底层错误，则返回错误
    fn find(&self, name: &str) -> Result<Option<Self>, VfsError> {
        let Fat32Entry::Dir(dir) = self else {
            return Err(VfsError::InvalidType);
        };
        for entry in dir.iter() {
            let entry = entry.map_err(VfsError::FsError)?;
            if entry.file_name() == name {
                if entry.is_dir() {
                    return Ok(Some(Fat32Entry::Dir(entry.to_dir())));
                } else {
                    return Ok(Some(Fat32Entry::File(entry.to_file())));
                }
            }
        }
        Ok(None)
    }
    /// 创建文件，若已存在则只是打开
    fn create_file(&self, name: &str) -> Result<Self, VfsError> {
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
    /// 创建文件，若已存在则只是打开
    fn create_dir(&self, name: &str) -> Result<Self, VfsError> {
        let dir = match self {
            Fat32Entry::Dir(root) => root,
            Fat32Entry::File(_) => {
                return Err(VfsError::InvalidType);
            }
        };
        match dir.create_dir(name) {
            Ok(dir) => Ok(Fat32Entry::Dir(dir)),
            Err(e) => Err(VfsError::FsError(e)),
        }
    }
    /// 清空文件内容，或者说截断到 0
    fn clear(&mut self) -> bool {
        match self {
            Fat32Entry::Dir(_) => false,
            Fat32Entry::File(file) => {
                file.seek(SeekFrom::Start(0)).unwrap();
                file.truncate().is_ok()
            }
        }
    }
    fn size(&self) -> u64 {
        match self {
            Fat32Entry::Dir(_) => 0,
            Fat32Entry::File(file) => file.size().map(u64::from).unwrap_or(0),
        }
    }
}

/// 一个利用缓存的磁盘驱动器。
///
/// TODO: 由于目前没有什么好的手段确定磁盘的总大小，假定读入时不会超过总大小
pub struct DiskDriver {
    device: &'static dyn BlockDevice,
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
            let nread = get_block_cache(self.block_id, self.device)
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
            let nwrite = get_block_cache(self.block_id, self.device)
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
