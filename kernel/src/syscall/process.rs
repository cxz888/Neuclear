//! Process management syscalls

use crate::{
    config::SIGSET_SIZE_BYTES,
    memory::{PageTable, VirtAddr},
    signal::{Signal, SignalAction, SignalSet},
    syscall::flags::{MmapFlags, MmapProt},
    task::{
        current_page_table, current_process, current_task, exit_current_and_run_next,
        suspend_current_and_run_next, CloneFlags, ThreadStatus,
    },
    utils::{
        error::{code, Result},
        structs::{TimeVal, UtsName},
        timer::get_time_us,
    },
};
use alloc::{string::String, sync::Arc, vec::Vec};
use num_enum::TryFromPrimitive;

/// 退出当前任务并设置其退出码为 `exit_code & 0xff`，该函数不返回
pub fn sys_exit(exit_code: i32) -> ! {
    // TODO: 退出需要给其父进程发送 `SIGCHLD` 信号
    exit_current_and_run_next(exit_code & 0xff)
}

/// 挂起当前任务，永不失败。返回 0
pub fn sys_yield() -> Result {
    suspend_current_and_run_next();
    Ok(0)
}

/// 返回当前进程 id，永不失败
pub fn sys_getpid() -> Result {
    Ok(current_process().pid() as isize)
}

/// 返回当前进程的父进程的 id，永不失败
pub fn sys_getppid() -> Result {
    Ok(current_process().inner().parent.upgrade().unwrap().pid() as isize)
}

/// 创建子任务，通过 flags 进行精确控制。
pub fn sys_clone(flags: usize, user_stack: usize, ptid: usize, tls: usize, ctid: usize) -> Result {
    // TODO: 完善 sys_clone()
    if u32::try_from(flags).is_err() {
        log::error!("flags 超过 u32：{flags:#b}");
        return Err(code::TEMP);
    }
    // 参考 https://man7.org/linux/man-pages/man2/clone.2.html，低 8 位是 exit_signal，其余是 clone flags
    let Some(clone_flags) = CloneFlags::from_bits((flags as u32) & !0xff) else {
        log::error!("未定义的 Clone Flags：{:#b}",flags & !0xff);
        return Err(code::TEMP);
    };
    let Ok(exit_signal) = Signal::try_from(flags as u8) else {
        log::error!("未定义的信号：{:#b}",flags as u8);
        return Err(code::TEMP);
    };
    if !clone_flags.is_empty() {
        log::error!("Clone Flags 包含暂未实现的内容：{clone_flags:?}");
        return Err(code::TEMP);
    }

    let current_process = current_process();
    let new_process = current_process.fork();
    let new_pid = new_process.pid();
    let new_process_inner = new_process.inner();
    let thread = new_process_inner.main_thread();
    let trap_ctx = thread.inner().trap_ctx();
    trap_ctx.x[10] = 0;
    Ok(new_pid as isize)
}

/// 将当前进程的地址空间清空并加载一个特定的可执行文件，返回用户态后开始它的执行。返回参数个数
///
/// 参数：
/// - `path` 给出了要加载的可执行文件的名字，必须以 `\0` 结尾
/// - `args` 给出了参数列表。其最后一个元素必须是一个 0
pub fn sys_exec(path: *const u8, mut args: *const usize) -> Result {
    let page_table = current_page_table();
    unsafe {
        let path = page_table.trans_str(path)?;
        let mut args_vec: Vec<String> = Vec::new();
        // 收集参数列表
        loop {
            let arg_str_ptr = *page_table.trans_ptr::<usize>(args)?;
            if arg_str_ptr == 0 {
                break;
            }
            args_vec.push(page_table.trans_str(arg_str_ptr as *const u8)?);
            args = args.add(1);
        }
        let process = current_process();
        let argc = args_vec.len();
        process.exec(path, args_vec)?;
        Ok(argc as isize)
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> Result {
    let process = current_process();

    // find a child process
    let mut inner = process.inner();
    let pair = inner
        .children
        .iter()
        .enumerate()
        .find(|(_, p)| p.inner().is_zombie && (pid == -1 || pid as usize == p.pid()));
    let (idx, _) = pair.ok_or(code::EAGAIN)?;
    let child = inner.children.remove(idx);
    // confirm that child will be deallocated after removing from children list
    assert_eq!(Arc::strong_count(&child), 1);
    let found_pid = child.pid();
    let exit_code = child.inner().exit_code;
    let mut pt = PageTable::from_token(inner.memory_set.token());
    unsafe {
        *pt.trans_ptr_mut(exit_code_ptr)? = exit_code;
    }
    Ok(found_pid as isize)
}

const MICRO_PER_SEC: usize = 1_000_000;

pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> Result {
    let mut pt = current_page_table();
    let ts = unsafe { pt.trans_ptr_mut(ts)? };
    let us = get_time_us();
    ts.sec = us / MICRO_PER_SEC;
    ts.usec = us % MICRO_PER_SEC;
    Ok(0)
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: ThreadStatus,
    pub time: usize,
}

pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    todo!()
}

pub fn sys_set_priority(_prio: isize) -> isize {
    todo!()
}

/// 映射虚拟内存。返回实际映射的地址。
///
/// `addr` 若是 NULL，那么内核会自动选择一个按页对齐的地址进行映射，这也是比较可移植的方式。
///
/// `addr` 若有指定地址，那么内核会尝试在最近的页边界上映射，但如果已经被映射过了，
/// 就挑选一个新的地址。该新地址可能参考也可能不参考 `addr`。
///
/// 如果映射文件，那么会以该文件 (`fd`) `offset` 开始处的 `len` 个字节初始化映射内容。
///
/// `mmap()` 返回之后，就算 `fd` 指向的文件被立刻关闭，也不会影响映射的结果
///
/// `prot` 要么是 `PROT_NONE`，要么是多个标志位的或。
///
/// `flags` 决定该映射是否对其他映射到同一区域的进程可见，以及更新是否会同步到底层文件上。
///
/// 参数：
/// - `addr` 映射的目标地址。
/// - `len` 映射的目标长度
/// - `prot` 描述该映射的内存保护方式，不能与文件打开模式冲突
/// - `flags` 描述映射的特征，详细参考 MmapFlags
/// - `fd` 被映射的文件描述符
/// - `offset` 映射的起始偏移，必须是 PAGE_SIZE 的整数倍
pub fn sys_mmap(addr: usize, len: usize, prot: u32, flags: u32, fd: i32, offset: usize) -> Result {
    log::debug!("addr: {addr}");
    log::debug!("len: {len}");

    if VirtAddr(addr).page_offset() != 0 || VirtAddr(len).page_offset() != 0 || len == 0 {
        return Err(code::EINVAL);
    }
    let Some(prot) = MmapProt::from_bits(prot) else {
        // prot 出现了意料之外的标志位
        log::error!("prot: {prot:#b}");
        return Err(code::TEMP);
    };
    let Some(flags) = MmapFlags::from_bits(flags) else {
        // flags 出现了意料之外的标志位
        log::error!("flags: {flags:#b}");
        return Err(code::TEMP);
    };
    log::debug!("prot: {prot:?}");
    log::debug!("flags: {flags:?}");
    log::debug!("fd: {fd}");
    log::debug!("offset: {offset}");
    if flags.contains(MmapFlags::MAP_ANONYMOUS | MmapFlags::MAP_SHARED) {
        log::error!("anonymous shared mapping is not supported!");
        return Err(code::EPERM);
    }
    if flags.contains(MmapFlags::MAP_ANONYMOUS) {
        if fd != -1 || offset != 0 {
            log::error!("fd must be -1 and offset must be 0 for anonyous mapping");
            return Err(code::EPERM);
        }
        let process = current_process();
        log::debug!("pid: {}", process.pid());
        let mut inner = process.inner();
        return inner.memory_set.try_map(
            VirtAddr(addr).vpn()..VirtAddr(addr + len).vpn(),
            prot.into(),
            false,
        );
    }

    todo!()
}

pub fn sys_munmap(addr: usize, len: usize) -> isize {
    todo!()
}

pub fn sys_spawn(_path: *const u8) -> Result {
    todo!()
    // let page_table = current_page_table();
    // let path = if let Some(path) = page_table.translate_str(path) {
    //     path
    // } else {
    //     return -1;
    // };
    // if let Some(app_inode) = open_file(&path, OpenFlags::RDONLY) {
    //     let current_process = current_process();
    //     current_process.spawn(&app_inode.read_all())
    // } else {
    //     -1
    // }
}

/// 设置线程控制块中 `clear_child_tid` 的值为 `tidptr`。总是返回调用者线程的 tid。
///
/// 参数：
/// - `tidptr`
pub fn sys_set_tid_address(tidptr: *const i32) -> Result {
    // NOTE: 在 linux 手册中，`tidptr` 的类型是 int*。这里设置为 i32，是参考 libc crate 设置 c_int=i32
    let thread = current_task().unwrap();
    let mut inner = thread.inner();
    inner.clear_child_tid = tidptr as usize;
    Ok(inner.res.as_ref().unwrap().tid as isize)
}

/// 将 program break 设置为 `brk`。高于当前堆顶会分配空间，低于则会释放空间。
///
/// `brk` 为 0 时返回当前堆顶地址。设置成功时返回新的 brk，设置失败返回原来的 brk
///
/// 参数：
/// - `brk` 希望设置的 program break 值
pub fn sys_brk(brk: usize) -> Result {
    let process = current_process();
    let mut inner = process.inner();
    // 不大于最初的堆地址则失败。其中也包括了 brk 为 0  的情况
    Ok(inner.set_user_brk(brk) as isize)
}

/// 为当前进程设置信号动作，返回 0
///
/// 参数：
/// - `signum` 指示信号编号，但不可以是 `SIGKILL` 或 `SIGSTOP`
/// - `act` 如果非空，则将信号 `signal` 的动作设置为它
/// - `old_act` 如果非空，则将信号 `signal` 原来的动作备份在其中
pub fn sys_sigaction(
    signum: usize,
    act: *const SignalAction,
    old_act: *mut SignalAction,
) -> Result {
    let signal = Signal::try_from_primitive(signum as u8).or(Err(code::EINVAL))?;
    // `SIGKILL` 和 `SIGSTOP` 的行为不可修改
    if matches!(signal, Signal::SIGKILL | Signal::SIGSTOP) {
        return Err(code::EINVAL);
    }
    let process = current_process();
    let mut inner = process.inner();
    let mut pt = PageTable::from_token(inner.user_token());

    if !old_act.is_null() {
        let old_act = unsafe { pt.trans_ptr_mut(old_act)? };
        *old_act = inner.sig_handlers.action(signal);
    }

    if !act.is_null() {
        let act = unsafe { pt.trans_ptr(act)? };
        inner.sig_handlers.set_action(signal, *act);
    }

    Ok(0)
}

/// 修改当前线程的信号掩码，返回 0
///
/// 参数：
/// - `how` 只应取 0(`SIG_BLOCK`)、1(`SIG_UNBLOCK`)、2(`SIG_SETMASK`)，表示函数的处理方式。
///     - `SIG_BLOCK` 向掩码 bitset 中添入新掩码
///     - `SIG_UNBLOCK` 从掩码 bitset 中取消掩码
///     - `SIG_SETMASK` 直接设置掩码 bitset
/// - `set` 为空时，信号掩码不会被修改（无论 `how` 取何值）。其余时候则是新掩码参数，根据 `how` 进行设置
/// - `old_set` 非空时，将旧掩码的值放入其中
pub fn sys_sigprocmask(
    how: usize,
    set: *const SignalSet,
    old_set: *mut SignalSet,
    sigsetsize: usize,
) -> Result {
    // NOTE: 这里 `set` == `old_set` 的情况是否需要考虑一下
    if sigsetsize != SIGSET_SIZE_BYTES {
        return Err(code::EINVAL);
    }
    let thread = current_task().unwrap();
    let mut inner = thread.inner();
    let mut pt = PageTable::from_token(thread.user_token());

    let sig_set = &mut inner.sig_receiver.mask;
    if !old_set.is_null() {
        unsafe {
            *pt.trans_ptr_mut(old_set)? = *sig_set;
        }
    }
    if set.is_null() {
        return Ok(0);
    }
    const SIG_BLOCK: usize = 0;
    const SIG_UNBLOCK: usize = 1;
    const SIG_SETMASK: usize = 2;
    let new_set = unsafe { *pt.trans_ptr(set)? };
    match how {
        SIG_BLOCK => {
            sig_set.insert(new_set);
        }
        SIG_UNBLOCK => {
            sig_set.remove(new_set);
        }
        SIG_SETMASK => {
            *sig_set = new_set;
        }
        _ => return Err(code::EINVAL),
    }

    Ok(0)
}

/// 返回系统信息，目前设计中永不失败，返回 0
pub fn sys_uname(utsname: *mut UtsName) -> Result {
    unsafe {
        *current_page_table().trans_ptr_mut(utsname)? = UtsName::default();
    }
    Ok(0)
}

/// 设置进程组号
///
/// TODO: 暂时未实现，仅返回 0
pub fn sys_setpgid(pid: usize, pgid: usize) -> Result {
    Ok(0)
}

/// 返回进程组号
///
/// TODO: 暂时未实现，仅返回 0
pub fn sys_getpgid(pid: usize) -> Result {
    Ok(0)
}
