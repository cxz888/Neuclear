use crate::driver_impl::BLOCK_DEVICE;
use crate::mm::UserBuffer;
use crate::sync::UPSafeCell;

use alloc::{sync::Arc, vec::Vec};
use bitflags::bitflags;
use fat32::{Fat32, Fat32Entry};
use lazy_static::lazy_static;
use vfs::{Entry, Fs};

type OsEntry = Fat32Entry;
type Vfs = Fat32;

/// A wrapper around a filesystem inode
/// to implement File trait atop
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}

/// The OS inode inner in 'UPSafeCell'
pub struct OSInodeInner {
    entry: OsEntry,
}

impl OSInode {
    /// Construct an OS inode from a inode
    pub fn new(readable: bool, writable: bool, entry: OsEntry) -> Self {
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner { entry }) },
        }
    }
    /// Read all data inside a inode into vector
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        inner.entry.read_all().unwrap()
    }
}

lazy_static! {
    /// The root of all inodes, or '/' in short
    pub static ref VIRTUAL_FS: Vfs = {
        Vfs::new(Arc::clone(&BLOCK_DEVICE))
    };
}

/// List all files in the filesystems
pub fn list_apps() {
    println!("/**** APPS ****");
    for app in VIRTUAL_FS.root_dir().ls().unwrap() {
        println!("{}", app);
    }
    println!("**************/");
}

bitflags! {
    /// Flags for opening files
    pub struct OpenFlags: u32 {
        const RDONLY = 0;
        const WRONLY = 1 << 0;
        const RDWR = 1 << 1;
        const CREATE = 1 << 9;
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// Get the current read write permission on an inode
    /// does not check validity for simplicity
    /// returns (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

/// 根据文件名打开一个根目录下的文件
///
/// TODO: 改进为支持任意路径
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    let root = VIRTUAL_FS.root_dir();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(mut inode) = root.find(name).unwrap() {
            inode.clear();
            Some(Arc::new(OSInode::new(readable, writable, inode)))
        } else {
            // create file
            let file = root.create(name).unwrap();
            Some(Arc::new(OSInode::new(readable, writable, file)))
        }
    } else {
        root.find(name).unwrap().map(|mut inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            Arc::new(OSInode::new(readable, writable, inode))
        })
    }
}

// impl File for OSInode {
//     fn readable(&self) -> bool {
//         self.readable
//     }
//     fn writable(&self) -> bool {
//         self.writable
//     }
//     fn read(&self, mut buf: UserBuffer) -> usize {
//         todo!()
//         // let mut inner = self.inner.exclusive_access();
//         // let mut total_read_size = 0usize;
//         // for slice in buf.buffers.iter_mut() {
//         //     let read_size = inner.inode.read_at(inner.offset, slice);
//         //     if read_size == 0 {
//         //         break;
//         //     }
//         //     inner.offset += read_size;
//         //     total_read_size += read_size;
//         // }
//         // total_read_size
//     }
//     fn write(&self, buf: UserBuffer) -> usize {
//         todo!();
//         // let mut inner = self.inner.exclusive_access();
//         // let mut total_write_size = 0usize;
//         // for slice in buf.buffers.iter() {
//         //     let write_size = inner.inode.write_at(inner.offset, slice);
//         //     assert_eq!(write_size, slice.len());
//         //     inner.offset += write_size;
//         //     total_write_size += write_size;
//         // }
//         // total_write_size
//     }
// }
