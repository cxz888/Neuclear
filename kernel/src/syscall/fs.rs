//! File and filesystem-related syscalls

use crate::fs::make_pipe;
// use crate::fs::open_file;
use crate::fs::Stat;
use crate::mm::translated_byte_buffer;
use crate::mm::PageTable;
use crate::mm::UserBuffer;
use crate::mm::VirtAddr;
use crate::task::current_process;
use crate::task::current_user_token;
use alloc::sync::Arc;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let process = current_process();
    let inner = process.inner_exclusive_access();

    if let Some(Some(file)) = inner.fd_table.get(fd) {
        let file = Arc::clone(file);
        let token = inner.user_token();
        assert!(file.writable());
        // write 有可能导致阻塞与任务切换
        drop(inner);
        drop(process);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    let process = current_process();
    let inner = process.inner_exclusive_access();

    if let Some(Some(file)) = inner.fd_table.get(fd) {
        let file = Arc::clone(&file.clone());
        let token = inner.user_token();
        assert!(file.readable());
        // read 有可能导致阻塞与任务切换
        drop(inner);
        drop(process);
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(_path: *const u8, _flags: u32) -> isize {
    todo!()
    // let ret = || -> Option<isize> {
    //     let path = translated_str(current_user_token(), path)?;
    //     let flags = OpenFlags::from_bits(flags)?;
    //     let inode = open_file(&path, flags)?;
    //     let process = current_process();
    //     let mut inner = process.inner_exclusive_access();
    //     let fd = inner.alloc_fd();
    //     inner.fd_table[fd] = Some(inode);
    //     Some(fd as isize)
    // }();
    // ret.unwrap_or(-1)
}

pub fn sys_close(fd: usize) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    match inner.fd_table.get(fd) {
        Some(Some(_)) => inner.fd_table[fd].take(),
        _ => return -1,
    };
    0
}

pub fn sys_pipe(pipe: *mut usize) -> isize {
    let process = current_process();
    let token = current_user_token();
    let mut inner = process.inner_exclusive_access();
    let (pipe_read, pipe_write) = make_pipe();
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(pipe_read);
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(pipe_write);
    let pt = PageTable::from_token(token);
    *pt.trans_va_as_mut(VirtAddr(pipe as usize)).unwrap() = read_fd;
    *pt.trans_va_as_mut(VirtAddr(unsafe { pipe.add(1) } as usize))
        .unwrap() = write_fd;
    0
}

pub fn sys_dup(fd: usize) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = Some(Arc::clone(inner.fd_table[fd].as_ref().unwrap()));
    new_fd as isize
}

pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    -1
}

pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    -1
}

pub fn sys_unlinkat(_name: *const u8) -> isize {
    -1
}
