//! File and filesystem-related syscalls

use crate::{
    fs::{make_pipe, open_file, OpenFlags, Stat},
    memory::{PageTable, UserBuffer, VirtAddr},
    task::{current_page_table, current_process, current_task, current_user_token},
    utils::error::{code, Result},
};
use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
};

/// 操纵某个文件的底层设备。目前只进行错误检验
///
/// 参数：
/// - `fd` 文件描述符
/// - `cmd` 请求码，其含义完全由底层设备决定
/// - `arg` 额外参数
pub fn sys_ioctl(fd: usize, _cmd: usize, arg: *mut usize) -> Result {
    if !matches!(current_process().inner().fd_table.get(fd), Some(Some(_))) {
        return Err(code::EBADF);
    }
    if current_page_table()
        .trans_va_to_pa(VirtAddr(arg as usize))
        .is_none()
    {
        return Err(code::EFAULT);
    }
    Ok(0)
}

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> Result {
    let process = current_process();
    let inner = process.inner();

    if let Some(Some(file)) = inner.fd_table.get(fd) {
        let file = Arc::clone(file);
        let mut pt = PageTable::from_token(inner.user_token());
        assert!(file.writable());
        // write 有可能导致阻塞与任务切换
        drop(inner);
        drop(process);

        let nwrite = file.write(UserBuffer::new(unsafe { pt.trans_byte_buffer(buf, len) }));
        Ok(nwrite as isize)
    } else {
        Err(code::TEMP)
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> Result {
    let process = current_process();
    let inner = process.inner();

    if let Some(Some(file)) = inner.fd_table.get(fd) {
        let file = Arc::clone(&file.clone());
        let mut pt = PageTable::from_token(inner.user_token());
        assert!(file.readable());
        // read 有可能导致阻塞与任务切换
        drop(inner);
        drop(process);
        let nread = file.read(UserBuffer::new(unsafe { pt.trans_byte_buffer(buf, len) }));
        Ok(nread as isize)
    } else {
        Err(code::TEMP)
    }
}

/// 不太清楚 `./` 开头的路径怎么处理。会借用当前进程
fn path_with_fd(fd: usize, path_name: String) -> Result<String> {
    const AT_FDCWD: usize = -100isize as usize;
    // 绝对路径则忽视 fd
    if path_name.starts_with('/') {
        return Ok(path_name);
    }
    let process = current_process();
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
pub fn sys_openat(dir_fd: usize, path_name: *const u8, flags: u32, mode: u32) -> Result {
    let pt = current_page_table();
    let file_name = unsafe { pt.trans_str(path_name).ok_or(code::EFAULT)? };
    assert_eq!(mode, 0);

    let Some(flags) = OpenFlags::from_bits(flags) else {
        log::error!("open flags: {flags:#b}");
        return Err(code::TEMP);
    };

    // 64 位版本应当是保证可以打开大文件的
    assert!(flags.contains(OpenFlags::O_LARGEFILE));

    if flags.intersects(OpenFlags::O_ASYNC | OpenFlags::O_APPEND | OpenFlags::O_DSYNC) {
        log::error!("todo openflags: {flags:#b}");
        return Err(code::TEMP);
    }

    let absolute_path = path_with_fd(dir_fd, file_name)?;
    let inode = open_file(absolute_path, flags)?;
    let process = current_process();
    let mut inner = process.inner();
    let fd = inner.alloc_fd();
    inner.fd_table[fd] = Some(inode);
    Ok(fd as isize)
}

pub fn sys_close(fd: usize) -> Result {
    let process = current_process();
    let mut inner = process.inner();
    match inner.fd_table.get(fd) {
        Some(Some(_)) => inner.fd_table[fd].take(),
        _ => return Err(code::EBADF),
    };
    Ok(0)
}

// TODO: `sys_pipe` 还需要改进，使其返回 Result
pub fn sys_pipe(pipe: *mut usize) -> isize {
    let process = current_process();
    let token = current_user_token();
    let mut inner = process.inner();
    let (pipe_read, pipe_write) = make_pipe();
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(pipe_read);
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(pipe_write);
    let mut pt = PageTable::from_token(token);
    unsafe {
        *pt.trans_ptr_mut(pipe).unwrap() = read_fd;
        *pt.trans_ptr_mut(pipe.add(1)).unwrap() = write_fd;
    }
    0
}

/// 操控文件描述符
///
/// 参数：
/// - `fd` 是指定的文件描述符
/// - `cmd` 指定需要进行的操作
/// - `arg` 是该操作可选的参数
pub fn sys_fcntl64(fd: usize, cmd: usize, arg: usize) -> Result {
    const F_DUPFD: usize = 0;
    const F_DUPFD_CLOEXEC: usize = 1030;
    log::debug!("fd: {fd}, cmd: {cmd}, arg: {arg}");
    let process = current_process();
    let mut inner = process.inner();
    let Some(Some(fd))=inner.fd_table.get(fd) else {
        return Err(code::EBADF);
    };
    match cmd {
        F_DUPFD | F_DUPFD_CLOEXEC => {
            let new_fd = Arc::clone(fd);
            let pos = inner.alloc_fd_from(arg);
            if cmd == F_DUPFD_CLOEXEC {
                new_fd.set_close_on_exec(true);
            }
            inner.fd_table[pos] = Some(new_fd);
            Ok(pos as isize)
        }
        _ => {
            log::error!("unsupported cmd: {cmd}, with arg: {arg}");
            Err(code::TEMP)
        }
    }
}

pub fn sys_dup(fd: usize) -> Result {
    let process = current_process();
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

pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    -1
}

pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    -1
}

pub fn sys_unlinkat(_name: *const u8) -> isize {
    -1
}

/// 获取当前进程当前工作目录的绝对路径。
///
/// 参数：
/// - `buf` 用于写入路径，以 `\0` 表示字符串结尾
/// - `size` 如果路径（包括 `\0`）长度大于 `size` 则返回 ERANGE
pub fn sys_getcwd(mut buf: *mut u8, size: usize) -> Result {
    let process = current_process();
    let inner = process.inner();
    let cwd = &inner.cwd;
    if size <= cwd.len() {
        return Err(code::ERANGE);
    }
    let mut pt = PageTable::from_token(inner.user_token());
    unsafe {
        for &byte in cwd.as_bytes() {
            *pt.trans_ptr_mut(buf).ok_or(code::EFAULT)? = byte;
            buf = buf.add(1);
        }
        *pt.trans_ptr_mut(buf).ok_or(code::EFAULT)? = 0;
    }

    Ok(buf as isize)
}
