//! File and filesystem-related syscalls

use crate::task::{
    check_cstr, check_ptr_mut, check_slice, check_slice_mut, curr_page_table, curr_process,
};
use alloc::{
    borrow::ToOwned,
    format,
    string::{String, ToString},
    sync::Arc,
};
use filesystem::{open_file, File, OpenFlags, Stat};
use memory::VirtAddr;
use utils::error::{code, Result};

/// 操纵某个文件的底层设备。目前只进行错误检验
///
/// 参数：
/// - `fd` 文件描述符
/// - `cmd` 请求码，其含义完全由底层设备决定
/// - `arg` 额外参数
pub fn sys_ioctl(fd: usize, cmd: usize, arg: usize) -> Result {
    log::debug!("ioctl fd: {fd}, cmd: {cmd}, arg: {arg}");
    if !matches!(curr_process().inner().fd_table.get(fd), Some(Some(_))) {
        return Err(code::EBADF);
    }
    if curr_page_table().trans_va_to_pa(VirtAddr(arg)).is_none() {
        return Err(code::EFAULT);
    }
    Ok(0)
}

#[rustfmt::skip]
fn prepare_io(fd: usize, is_read: bool) -> Result<Arc<dyn File>> {
    let process = curr_process();
    let inner = process.inner();
    if let Some(Some(file)) = inner.fd_table.get(fd) && 
        ((is_read && file.readable()) || (!is_read&& file.writable()))
    {
        let file = Arc::clone(&file.clone());
        if file.is_dir() {
            return Err(code::EISDIR);
        }
        Ok(file)
    } else {
        Err(code::EBADF)
    }
}

/// 从 fd 指示的文件中读至多 `len` 字节的数据到用户缓冲区中。成功时返回读入的字节数
///
/// 参数：
/// - `fd` 指定的文件描述符，若无效则返回 `EBADF`，若是目录则返回 `EISDIR`
/// - `buf` 指定用户缓冲区，若无效则返回 `EINVAL`
/// - `len` 指定至多读取的字节数
pub fn sys_read(fd: usize, buf: *mut u8, len: usize) -> Result {
    let buf = unsafe { check_slice_mut(buf, len)? };
    let file = prepare_io(fd, true)?;
    let nread = file.read(buf);
    Ok(nread as isize)
}

/// 向 fd 指示的文件中写入至多 `len` 字节的数据。成功时返回写入的字节数
///
/// 参数：
/// - `fd` 指定的文件描述符，若无效则返回 `EBADF`，若是目录则返回 `EISDIR`
/// - `buf` 指定用户缓冲区，其中存放需要写入的内容，若无效则返回 `EINVAL`
/// - `len` 指定至多写入的字节数
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> Result {
    let buf = unsafe { check_slice(buf, len)? };
    let file = prepare_io(fd, false)?;
    let nwrite = file.write(buf);
    Ok(nwrite as isize)
}

#[repr(C)]
pub struct IoVec {
    iov_base: *mut u8,
    iov_len: usize,
}

/// 从 fd 中读入数据，写入多个用户缓冲区中。
///
/// 理论上需要保证原子性，也就是说，即使同时有其他进程（线程）对同一个 fd 进行读操作，
/// 这一个系统调用也会读入一块连续的区域。目前未实现。
///
/// 参数：
/// - `fd` 指定文件描述符
/// - `iovec` 指定 `IoVec` 数组
/// - `vlen` 指定数组的长度
pub fn sys_readv(fd: usize, iovec: *const IoVec, vlen: usize) -> Result {
    let iovec = unsafe { check_slice(iovec, vlen)? };
    let file = prepare_io(fd, true)?;
    let mut tot_read = 0;
    for iov in iovec {
        let buf = unsafe { check_slice_mut(iov.iov_base, iov.iov_len)? };
        let nread = file.read(buf);
        if nread == 0 {
            break;
        }
        tot_read += nread;
    }
    Ok(tot_read as isize)
}

/// 向 fd 中写入数据，数据来自多个用户缓冲区。
///
/// 理论上需要保证原子性，也就是说，即使同时有其他进程（线程）对同一个 fd 进行写操作，
/// 这一个系统调用也会写入一块连续的区域。目前未实现。
///
/// 参数：
/// - `fd` 指定文件描述符
/// - `iovec` 指定 `IoVec` 数组
/// - `vlen` 指定数组的长度
pub fn sys_writev(fd: usize, iovec: *const IoVec, vlen: usize) -> Result {
    let iovec = unsafe { check_slice(iovec, vlen)? };
    let file = prepare_io(fd, true)?;
    let mut total_write = 0;
    for iov in iovec {
        let buf = unsafe { check_slice(iov.iov_base, iov.iov_len)? };
        let nwrite = file.write(buf);
        if nwrite == 0 {
            break;
        }
        total_write += nwrite;
    }
    Ok(total_write as isize)
}

/// 不太清楚 `./` 开头的路径怎么处理。会借用当前进程
fn path_with_fd(fd: usize, path_name: String) -> Result<String> {
    const AT_FDCWD: usize = -100isize as usize;
    // 绝对路径则忽视 fd
    if path_name.starts_with('/') {
        return Ok(path_name);
    }
    let process = curr_process();
    let inner = process.inner();
    if fd == AT_FDCWD {
        return Ok(format!("{}/{path_name}", inner.cwd));
    } else {
        if let Some(Some(base)) = inner.fd_table.get(fd) {
            if base.is_dir() {
                return Ok(format!("{}/{path_name}", base.path().unwrap().to_string()));
            } else {
                return Err(code::ENOTDIR);
            }
        } else {
            return Err(code::EBADF);
        }
    }
}

/// 打开指定的文件。返回非负的文件描述符，这个文件描述符一定是当前进程尚未打开的最小的那个
///
/// 参数：
/// - `dir_fd` 与 `path_name` 组合形成最终的路径。
///     - 若 `path_name` 本身是绝对路径，则忽略。
///     - 若 `dir_fd` 等于 `AT_FDCWD`(-100)
/// - `path_name` 路径，可以是绝对路径 (/xxx/yyy) 或相对路径 (xxx/yyy) 以 `/` 为分隔符
/// - `flags` 包括文件打开模式、创建标志、状态标志。
///     - 创建标志如 `O_CLOEXEC`, `O_CREAT` 等，仅在打开文件时发生作用
///     - 状态标志影响后续的 I/O 方式，而且可以动态修改
/// - `mode` 是用于指定创建新文件时，该文件的 mode。目前应该不会用到
///     - 它只会影响未来访问该文件的模式，但这一次打开该文件可以是随意的
pub fn sys_openat(dir_fd: usize, path_name: *const u8, flags: u32, mut mode: u32) -> Result {
    let file_name = unsafe { check_cstr(path_name)? };

    let Some(flags) = OpenFlags::from_bits(flags) else {
        log::error!("open flags: {flags:#b}");
        return Err(code::TEMP);
    };
    log::info!("oepnat {dir_fd}, {file_name}, {flags:?}");
    // 不是创建文件（以及临时文件）时，mode 被忽略
    if !flags.contains(OpenFlags::O_CREAT) {
        mode = 0;
    }
    // TODO: 暂时在测试中忽略
    #[cfg(not(feature = "test"))]
    assert_eq!(mode, 0, "dir_fd: {dir_fd}, flags: {flags:?}");

    // 64 位版本应当是保证可以打开大文件的
    // TODO: 暂时在测试中忽略
    #[cfg(not(feature = "test"))]
    assert!(flags.contains(OpenFlags::O_LARGEFILE));

    // 暂时先不支持这些
    if flags.intersects(OpenFlags::O_ASYNC | OpenFlags::O_APPEND | OpenFlags::O_DSYNC) {
        log::error!("todo openflags: {flags:#b}");
        return Err(code::TEMP);
    }

    let absolute_path = path_with_fd(dir_fd, file_name.to_owned())?;
    let inode = open_file(absolute_path, flags)?;
    let process = curr_process();
    let mut inner = process.inner();
    let fd = inner.alloc_fd();
    inner.fd_table[fd] = Some(inode);
    Ok(fd as isize)
}

pub fn sys_close(fd: usize) -> Result {
    let process = curr_process();
    let mut inner = process.inner();
    match inner.fd_table.get(fd) {
        Some(Some(_)) => inner.fd_table[fd].take(),
        _ => return Err(code::EBADF),
    };
    Ok(0)
}

pub fn sys_pipe2(_pipe: *mut usize) -> Result {
    // let process = current_process();
    // let token = current_user_token();
    // let mut inner = process.inner();
    // let (pipe_read, pipe_write) = make_pipe();
    // let read_fd = inner.alloc_fd();
    // inner.fd_table[read_fd] = Some(pipe_read);
    // let write_fd = inner.alloc_fd();
    // inner.fd_table[write_fd] = Some(pipe_write);
    // let mut pt = PageTable::from_token(token);
    // unsafe {
    //     *pt.trans_ptr_mut(pipe).unwrap() = read_fd;
    //     *pt.trans_ptr_mut(pipe.add(1)).unwrap() = write_fd;
    // }
    todo!("sys_pipe2 未实现")
}

/// 操控文件描述符
///
/// 参数：
/// - `fd` 是指定的文件描述符
/// - `cmd` 指定需要进行的操作
/// - `arg` 是该操作可选的参数
pub fn sys_fcntl64(fd: usize, cmd: usize, arg: usize) -> Result {
    const F_DUPFD: usize = 0;
    const F_GETFD: usize = 1;
    const F_SETFD: usize = 2;
    const F_DUPFD_CLOEXEC: usize = 1030;

    let process = curr_process();
    let mut inner = process.inner();
    let Some(Some(file))=inner.fd_table.get(fd) else {
        return Err(code::EBADF);
    };
    match cmd {
        F_DUPFD | F_DUPFD_CLOEXEC => {
            let file = Arc::clone(file);
            let new_fd = inner.alloc_fd_from(arg);
            if cmd == F_DUPFD_CLOEXEC {
                file.set_close_on_exec(true);
            }
            inner.fd_table[new_fd] = Some(file);
            Ok(new_fd as isize)
        }
        F_GETFD => {
            if file.status().contains(OpenFlags::O_CLOEXEC) {
                Ok(1)
            } else {
                Ok(0)
            }
        }
        F_SETFD => {
            file.set_close_on_exec(arg & 1 != 0);
            Ok(0)
        }
        _ => {
            log::error!("unsupported cmd: {cmd}, with arg: {arg}");
            Err(code::EINVAL)
        }
    }
}

pub fn sys_dup(fd: usize) -> Result {
    let process = curr_process();
    let mut inner = process.inner();
    if fd >= inner.fd_table.len() {
        return Err(code::TEMP);
    }
    if inner.fd_table[fd].is_none() {
        return Err(code::TEMP);
    }
    let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = Some(Arc::clone(inner.fd_table[fd].as_ref().unwrap()));
    Ok(new_fd as isize)
}

/// TODO: 写 sys_fstatat 的文档
pub fn sys_fstatat(dir_fd: usize, file_name: *const u8, statbuf: *mut Stat, flag: usize) -> Result {
    // TODO: 暂时先不考虑 fstatat 的 flags
    assert_eq!(flag, 0);
    let file_name = unsafe { check_cstr(file_name)? };
    log::info!("fstatat {dir_fd}, {file_name}");
    let absolute_path = path_with_fd(dir_fd, file_name.to_string())?;
    log::info!("absolute path: {absolute_path}");

    // TODO: 注意，可以尝试用 OpenFlags::O_PATH 打开试试
    let file = open_file(absolute_path, OpenFlags::empty())?;

    let statbuf = unsafe { check_ptr_mut(statbuf)? };
    *statbuf = file.fstat();

    Ok(0)
}

pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> Result {
    todo!()
}

pub fn sys_unlinkat(_name: *const u8) -> Result {
    todo!()
}

/// 获取当前进程当前工作目录的绝对路径。
///
/// 参数：
/// - `buf` 用于写入路径，以 `\0` 表示字符串结尾
/// - `size` 如果路径（包括 `\0`）长度大于 `size` 则返回 ERANGE
pub fn sys_getcwd(buf: *mut u8, size: usize) -> Result {
    let process = curr_process();
    let inner = process.inner();
    let cwd = &inner.cwd;
    // 包括 '\0'
    let buf_len = cwd.len() + 1;
    if buf_len > size {
        return Err(code::ERANGE);
    }
    {
        let buf = unsafe { check_slice_mut(buf, buf_len)? };
        buf[..buf_len - 1].copy_from_slice(cwd.as_bytes());
        buf[buf_len - 1] = 0;
    }
    Ok(buf as isize)
}

/// 等待文件描述符上的事件
///
/// TODO: 暂不实现 ppoll
pub fn sys_ppoll() -> Result {
    Ok(1)
}
