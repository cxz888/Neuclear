#![no_std]

mod inode;
mod pipe;
mod stdio;

extern crate alloc;
#[macro_use]
extern crate utils;

pub use inode::{list_apps, open_inode, InodeFile, OpenFlags};
pub use pipe::{make_pipe, Pipe};
pub use stdio::{Stdin, Stdout};

use alloc::{string::String, sync::Arc};
use bitflags::bitflags;
use memory::UserBuffer;
use utils::{
    error::{code, Result},
    time::TimeSpec,
};

extern "C" {
    fn __suspend_current_and_run_next();
}

/// The common abstraction of all IO resources
pub trait File: Send + Sync {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(&self, buf: UserBuffer) -> usize;
    fn write(&self, buf: UserBuffer) -> usize;
    fn set_close_on_exec(&self, _bit: bool) {}
    fn status(&self) -> OpenFlags {
        OpenFlags::empty()
    }
    fn is_dir(&self) -> bool {
        false
    }
    fn path(&self) -> Option<&str> {
        None
    }
    fn fstat(&self) -> Stat;
}

/// The stat of a inode
#[repr(C)]
#[derive(Debug, Default)]
pub struct Stat {
    /// 包含该文件的设备号
    pub st_dev: u64,
    /// inode 编号
    pub st_ino: u64,
    /// 文件类型和模式
    pub st_mode: StatMode,
    /// 硬链接的数量
    pub st_nlink: u32,
    /// Owner 的用户 ID
    pub st_uid: u32,
    /// Owner 的组 ID
    pub st_gid: u32,
    /// 特殊文件的设备号
    pub st_rdev: u64,
    _pad0: u64,
    /// 文件总大小
    pub st_size: u64,
    /// 文件系统 I/O 的块大小。
    ///
    /// TODO: 特殊文件也先填成 BLOCK_SIZE 吧
    pub st_blksize: u32,
    _pad1: u32,
    /// 已分配的 512Byte 块个数。
    ///
    /// TODO: 文件有空洞时，可能小于 st_size/512，暂且先不填吧
    pub st_blocks: u64,
    /// 最后一次访问时间 (Access TIME)
    pub st_atime: TimeSpec,
    /// 最后一次修改内容时间 (Modify TIME)
    pub st_mtime: TimeSpec,
    /// 最后一次改变状态时间 (Change TIME)
    pub st_ctime: TimeSpec,
    // ctime 在修改内容、属性时都会改变，而 mtime 只会在修改内容时改变
    // TODO: 非 Inode 文件的 time 属性该怎么处理？目前就是默认为 0
}

bitflags! {
    /// The mode of a inode
    /// whether a directory or a file
    #[derive(Clone, Copy, Debug, Default)]
    pub struct StatMode: u32 {
        // 以下类型只为其一
        /// 是普通文件
        const S_IFREG  = 1 << 15;
        /// 是符号链接
        const S_IFLNK  = 1 << 15 | 1 << 13;
        /// 是 socket
        const S_IFSOCK = 1 << 15 | 1<< 14;
        /// 是块设备
        const S_IFBLK  = 1 << 14 | 1 << 13;
        /// 是目录
        const S_IFDIR  = 1 << 14;
        /// 是字符设备
        const S_IFCHR  = 1 << 13;
        /// 是 FIFO
        const S_IFIFO  = 1 << 12;

        /// 是否设置 uid/gid/sticky
        // const S_ISUID = 1 << 11;
        // const S_ISGID = 1 << 10;
        // const S_ISVTX = 1 << 9;
        // TODO: 由于暂时没有权限系统，目前全设为 777
        /// 所有者权限
        const S_IRWXU = Self::S_IRUSR.bits() | Self::S_IWUSR.bits() | Self::S_IXUSR.bits();
        const S_IRUSR = 1 << 8;
        const S_IWUSR = 1 << 7;
        const S_IXUSR = 1 << 6;
        /// 用户组权限
        const S_IRWXG = Self::S_IRGRP.bits() | Self::S_IWGRP.bits() | Self::S_IXGRP.bits();
        const S_IRGRP = 1 << 5;
        const S_IWGRP = 1 << 4;
        const S_IXGRP = 1 << 3;
        /// 其他用户权限
        const S_IRWXO = Self::S_IROTH.bits() | Self::S_IWOTH.bits() | Self::S_IXOTH.bits();
        const S_IROTH = 1 << 2;
        const S_IWOTH = 1 << 1;
        const S_IXOTH = 1 << 0;
    }
}

/// 根据路径打开一个文件。包括特殊文件
pub fn open_file(path: String, flags: OpenFlags) -> Result<Arc<dyn File>> {
    if path.starts_with("/dev") {
        match path.as_str() {
            // NOTE: 暂时而言是这么实现的，实际上直接返回当前 fd_table 的 Stdout 行不行？
            "/dev/tty" => return Ok(Arc::new(Stdout)),
            _ => return Err(code::ENOENT),
        }
    }
    let inode = open_inode(path, flags)?;
    Ok(Arc::new(inode))
}
