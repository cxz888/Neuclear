use super::{File, Stat, StatMode};
use alloc::{string::String, vec::Vec};
use bitflags::bitflags;
use drivers::{BLOCK_DEVICE, BLOCK_SIZE};
use fat32::{Fat32, Fat32Entry};
use lazy_static::lazy_static;
use utils::{
    error::{code, Result},
    upcell::UPSafeCell,
};
use vfs::{Entry, Fs};

type OsEntry = Fat32Entry;
type Vfs = Fat32;

pub struct InodeFile {
    readable: bool,
    writable: bool,
    path: String,
    inner: UPSafeCell<InodeFileInner>,
}

/// The OS inode inner in 'UPSafeCell'
pub struct InodeFileInner {
    entry: OsEntry,
    flags: OpenFlags,
}

impl InodeFile {
    /// Construct an OS inode from a inode
    pub fn new(path: String, readable: bool, writable: bool, entry: OsEntry) -> Self {
        Self {
            path,
            readable,
            writable,
            inner: unsafe {
                UPSafeCell::new(InodeFileInner {
                    entry,
                    flags: OpenFlags::empty(),
                })
            },
        }
    }
    /// Read all data inside a inode into vector
    pub fn read_all(&self) -> Result<Vec<u8>> {
        let mut inner = self.inner.exclusive_access();
        inner.entry.read_all().map_err(|_| code::EISDIR)
    }
}

lazy_static! {
    /// The root of all inodes, or '/' in short
    pub static ref VIRTUAL_FS: Vfs = {
        Vfs::new(&*BLOCK_DEVICE)
    };
}

bitflags! {
    /// 注意低 2 位指出文件的打开模式
    /// 0、1、2 分别对应只读、只写、可读可写。3 为错误。
    #[derive(Clone, Copy, Debug)]
    pub struct OpenFlags: u32 {
        const O_RDONLY      = 0;
        const O_WRONLY      = 1 << 0;
        const O_RDWR        = 1 << 1;

        /// 如果所查询的路径不存在，则在该路径创建一个常规文件
        const O_CREAT       = 1 << 6;
        /// 在创建文件的情况下，保证该文件之前已经已存在，否则返回错误
        const O_EXCL        = 1 << 7;
        /// 如果路径指向一个终端设备，那么它不会称为本进程的控制终端
        const O_NOCTTY      = 1 << 8;
        /// 如果是常规文件，且允许写入，则将该文件长度截断为 0
        const O_TRUNC       = 1 << 9;
        /// 写入追加到文件末尾，可能在每次 `sys_write` 都有影响，暂时不支持
        const O_APPEND      = 1 << 10;
        /// 保持文件数据与磁盘阻塞同步。但如果该写操作不影响读取刚写入的数据，则不会等到元数据更新，暂不支持
        const O_DSYNC       = 1 << 12;
        /// 文件操作完成时发出信号，暂时不支持
        const O_ASYNC       = 1 << 13;
        /// 不经过缓存，直接写入磁盘中。目前实现仍然经过缓存
        const O_DIRECT      = 1 << 14;
        /// 允许打开文件大小超过 32 位表示范围的大文件。在 64 位系统上此标志位应永远为真
        const O_LARGEFILE   = 1 << 15;
        /// 如果打开的文件不是目录，那么就返回失败
        ///
        /// FIXME: 在测试中，似乎 1 << 21 才被认为是 O_DIRECTORY；但 musl 似乎认为是 1 << 16
        const O_DIRECTORY   = 1 << 21;
        // /// 如果路径的 basename 是一个符号链接，则打开失败并返回 `ELOOP`，目前不支持
        // const O_NOFOLLOW    = 1 << 17;
        // /// 读文件时不更新文件的 last access time，暂不支持
        // const O_NOATIME     = 1 << 18;
        /// 设置打开的文件描述符的 close-on-exec 标志
        const O_CLOEXEC     = 1 << 19;
        // /// 仅打开一个文件描述符，而不实际打开文件。后续只允许进行纯文件描述符级别的操作
        // TODO: 可能要考虑加上 O_PATH，似乎在某些情况下无法打开的文件可以通过它打开
        // const O_PATH        = 1 << 21;
    }
}

impl OpenFlags {
    /// Get the current read write permission on an inode
    /// does not check validity for simplicity
    /// returns (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        match self.bits() & 0b11 {
            0 => (true, false),
            1 => (false, true),
            2 => (true, true),
            _ => unreachable!(),
        }
    }
}

// TODO: 现在是什么方法都往 File 里面塞，感觉不好，未来要不要弄个 Any 之类的进行 downcast
impl File for InodeFile {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(&self, buf: &mut [u8]) -> usize {
        self.inner.exclusive_access().entry.read(buf).unwrap()
    }
    fn write(&self, buf: &[u8]) -> usize {
        self.inner.exclusive_access().entry.write(buf).unwrap()
    }
    fn set_close_on_exec(&self, bit: bool) {
        self.inner
            .exclusive_access()
            .flags
            .set(OpenFlags::O_CLOEXEC, bit);
    }
    fn status(&self) -> OpenFlags {
        self.inner.exclusive_access().flags
    }
    fn is_dir(&self) -> bool {
        self.inner.exclusive_access().entry.is_dir()
    }
    fn remove(&self, name: &str) {
        self.inner.exclusive_access().entry.remove(name).unwrap();
    }
    fn path(&self) -> Option<&str> {
        Some(&self.path)
    }
    fn fstat(&self) -> Stat {
        let inner = self.inner.exclusive_access();
        let st_size = inner.entry.size();
        // FAT32 没有 inode 的概念，因此设为 1 即可；同时不支持链接，所以 nlink 直接设为 1
        // TODO: FAT32 的时间十分粗略，所以暂时先不考虑时间了
        Stat {
            st_dev: 1,
            st_ino: 1,
            st_nlink: 1,
            st_size,
            st_mode: StatMode::S_IFIFO | StatMode::S_IRWXU | StatMode::S_IRWXG | StatMode::S_IRWXO,
            st_blksize: BLOCK_SIZE,
            ..Default::default()
        }
    }
}

/// 打开一个磁盘上的文件
pub fn open_inode(path: String, flags: OpenFlags) -> Result<InodeFile> {
    let (readable, writable) = flags.read_write();
    let mut curr = VIRTUAL_FS.root_dir();
    if path == "/" {
        return Ok(InodeFile::new(path, readable, writable, curr));
    }
    let mut path_split = path.strip_prefix('/').unwrap_or(&path).split('/');
    // 能够完成这个循环说明该文件是存在的
    while let Some(name) = path_split.next() {
        log::debug!("component name: {name}");
        match curr.find(name) {
            Ok(Some(next)) => {
                curr = next;
            }
            Ok(None) => {
                // 最后一节路径未找到，若有 O_CREAT 则创建；否则返回 ENOENT
                if path_split.next().is_none() && flags.contains(OpenFlags::O_CREAT) {
                    log::debug!("try create");
                    // NOTE: 用 O_DIRECTORY 来标记是否创建目录了，这是否语义不正确呢？
                    let file = if flags.contains(OpenFlags::O_DIRECTORY) {
                        curr.create_dir(name).unwrap()
                    } else {
                        curr.create_file(name).unwrap()
                    };
                    return Ok(InodeFile::new(path, readable, writable, file));
                } else {
                    return Err(code::ENOENT);
                }
            }
            // 当前节点不是目录
            Err(vfs::Error::InvalidType) => {
                return Err(code::ENOTDIR);
            }
            Err(e) => {
                panic!("文件系统内部错误：{e:?}");
            }
        }
    }
    // 文件存在，但要求必须要创建
    if flags.contains(OpenFlags::O_CREAT | OpenFlags::O_EXCL) {
        return Err(code::EEXIST);
    }
    // 文件存在，但要求必须是目录
    if flags.contains(OpenFlags::O_DIRECTORY) && !curr.is_dir() {
        return Err(code::ENOTDIR);
    }
    if flags.contains(OpenFlags::O_TRUNC) && writable {
        curr.clear();
    }
    Ok(InodeFile::new(path, readable, writable, curr))
}
