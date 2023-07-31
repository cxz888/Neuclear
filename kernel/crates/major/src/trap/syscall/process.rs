//! Process management syscalls

use super::flags::{MmapFlags, MmapProt};
use crate::{
    task::{
        __exit_curr_and_run_next, __suspend_curr_and_run_next, check_cstr, check_ptr,
        check_ptr_mut, curr_process, curr_task, CloneFlags, ThreadStatus,
    },
    trap::syscall::flags::WaitFlags,
};
use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use memory::VirtAddr;
use num_enum::TryFromPrimitive;
use signal::{Signal, SignalAction, SignalSet};
use utils::{
    config::SIGSET_SIZE_BYTES,
    error::{code, Result},
    time::{get_time_ns, get_time_us, MICRO_PER_SEC, NANO_PER_SEC},
};
use utils::{
    structs::UtsName,
    time::{TimeSpec, TimeVal},
};

/// 退出当前任务并设置其退出码为 `exit_code & 0xff`，该函数不返回
pub fn sys_exit(exit_code: i32) -> ! {
    // TODO: 退出需要给其父进程发送 `SIGCHLD` 信号
    __exit_curr_and_run_next(exit_code & 0xff)
}

/// 挂起当前任务，让出 CPU，永不失败
pub fn sys_yield() -> Result {
    __suspend_curr_and_run_next();
    Ok(0)
}

/// 返回当前进程 id，永不失败
pub fn sys_getpid() -> Result {
    Ok(curr_process().pid() as isize)
}

/// 返回当前进程的父进程的 id，永不失败
pub fn sys_getppid() -> Result {
    Ok(curr_process().inner().parent.upgrade().unwrap().pid() as isize)
}

/// 创建子任务，通过 flags 进行精确控制。
///
/// TODO: 完善 sys_clone() 并写文档
pub fn sys_clone(
    flags: usize,
    _user_stack: usize,
    _ptid: usize,
    _tls: usize,
    _ctid: usize,
) -> Result {
    if u32::try_from(flags).is_err() {
        log::error!("flags 超过 u32：{flags:#b}");
        return Err(code::TEMP);
    }
    // 参考 https://man7.org/linux/man-pages/man2/clone.2.html，低 8 位是 exit_signal，其余是 clone flags
    let Some(clone_flags) = CloneFlags::from_bits((flags as u32) & !0xff) else {
        log::error!("未定义的 Clone Flags：{:#b}", flags & !0xff);
        return Err(code::TEMP);
    };
    // TODO: 完成 exit_signal
    let Ok(_exit_signal) = Signal::try_from(flags as u8) else {
        log::error!("未定义的信号：{:#b}", flags as u8);
        return Err(code::TEMP);
    };
    if !clone_flags.is_empty() {
        log::error!("Clone Flags 包含暂未实现的内容：{clone_flags:?}");
        return Err(code::TEMP);
    }

    let current_process = curr_process();
    let new_process = current_process.fork();
    let new_pid = new_process.pid();
    let new_process_inner = new_process.inner();
    let thread = new_process_inner.main_thread();
    let trap_ctx = unsafe { thread.inner().trap_ctx() };
    trap_ctx.x[10] = 0;
    Ok(new_pid as isize)
}

/// 将当前进程的地址空间清空并加载一个特定的可执行文件，返回用户态后开始它的执行。返回参数个数
///
/// 参数：
/// - `pathname` 给出了要加载的可执行文件的名字，必须以 `\0` 结尾
/// - `argv` 给出了参数列表。其最后一个元素必须是 0
/// - `envp` 给出环境变量列表，其最后一个元素必须是 0，目前未实现
pub fn sys_execve(pathname: *const u8, mut argv: *const usize, envp: *const usize) -> Result {
    // TODO: 暂时在测试中忽略
    #[cfg(not(feature = "test"))]
    assert!(envp.is_null(), "envp 暂时尚未支持");
    let pathname = unsafe { check_cstr(pathname)? };
    // 收集参数列表
    let mut arg_vec: Vec<String> = Vec::new();
    unsafe {
        while *argv != 0 {
            let arg_str = check_cstr(*argv as _)?;
            arg_vec.push(arg_str.to_string());
            argv = argv.add(1);
        }
    }
    // 执行新进程
    let process = curr_process();

    let argc = arg_vec.len();
    process.exec(pathname.to_string(), arg_vec)?;
    Ok(argc as isize)
}

/// 挂起本线程，等待子进程改变状态（终止、或信号处理）。默认而言，会阻塞式等待子进程终止。
///
/// 若成功，返回子进程 pid，若 `options` 指定了 WNOHANG 且子线程存在但状态为改变，则返回 0
///
/// TODO: 信号处理的部分暂未实现
///
/// 参数：
/// - `pid` 要等待的 pid
///     - `pid` < -1，则等待一个 pgid 为 `pid` 绝对值的子进程，目前不支持
///     - `pid` == -1，则等待任意一个子进程
///     - `pid` == 0，则等待一个 pgid 与调用进程**调用时**的 pgid 相同的子进程，目前不支持
///     - `pid` > 0，则等待指定 `pid` 的子进程
/// - `wstatus` 指向一个 int，若非空则用于表示某些状态，目前而言似乎仅需往里写入子进程的 exit code
/// - `options` 控制等待方式，详细查看 [`WaitFlags`]，目前只支持 `WNOHANG`
/// - `rusgae` 用于统计子进程资源使用情况，目前不支持
pub fn sys_wait4(pid: isize, wstatus: *mut i32, options: usize, rusage: usize) -> Result {
    assert!(
        pid == -1 || pid > 0,
        "pid < -1 和 pid == 0，也就是等待进程组，目前还不支持"
    );
    assert_eq!(rusage, 0, "目前 rusage 尚不支持，所以必须为 nullptr");
    let options = WaitFlags::from_bits(options as u32).ok_or(code::EINVAL)?;
    if options.contains(WaitFlags::WIMTRACED) || options.contains(WaitFlags::WCONTINUED) {
        log::error!("暂时仅支持 WNOHANG");
        return Err(code::UNSUPPORTED);
    }

    // 尝试寻找符合条件的子进程
    loop {
        let process = curr_process();
        let mut inner = process.inner();

        // 是否有符合 pid 要求的子进程
        let mut has_proper_child = false;
        let mut child_index = None;

        for (index, child) in inner.children.iter().enumerate() {
            if pid == -1 || child.pid() == pid as usize {
                has_proper_child = true;
                if child.inner().is_zombie {
                    child_index = Some(index);
                }
            }
        }

        // 有 pid 符合要求的子进程
        if has_proper_child {
            // 同时也已经退出的，则可以收集资源并返回
            if let Some(index) = child_index {
                let child = inner.children.remove(index);
                // 此时理论上子进程只在当前进程的子进程列表中保存了
                assert_eq!(Arc::strong_count(&child), 1);
                let found_pid = child.pid();
                let exit_code = child.inner().exit_code;
                if !wstatus.is_null() {
                    let wstatus = unsafe { check_ptr_mut(wstatus)? };
                    // *wstatus 的构成，可能要参考 WEXITSTATUS 那几个宏
                    *wstatus = exit_code << 8;
                }
                return Ok(found_pid as isize);
            } else {
                // 否则视 `options` 而定
                if options.contains(WaitFlags::WNOHANG) {
                    return Ok(0);
                } else {
                    drop(inner);
                    drop(process);
                    __suspend_curr_and_run_next();
                }
            }
        } else {
            return Err(code::ECHILD);
        }
    }
}

/// 获取自 Epoch 以来所过的时间（不过目前实现中似乎是自开机或复位以来时间）
///
/// 参数：
/// - `ts` 要设置的时间值
/// - `tz` 时区结构，但目前已经过时，不考虑
pub fn sys_get_time_of_day(tv: *mut TimeVal, _tz: usize) -> Result {
    // 根据 man 所言，时区参数 tz 已经过时了，通常应当是 NULL。
    assert_eq!(_tz, 0);
    let tv = unsafe { check_ptr_mut(tv)? };
    let us = get_time_us();
    tv.sec = us / MICRO_PER_SEC;
    tv.usec = us % MICRO_PER_SEC;
    Ok(0)
}

/// 全局时钟，或者说挂钟
const CLOCK_REALTIME: usize = 0;

/// 同样是获取时间，不过 `TimeSpec` 精度为 ns。
///
/// 可以有不同的时钟，但目前只支持挂钟 (`CLOCK_REALTIME`)。
///
/// 参数：
/// - `clock_id` 时钟 id，目前仅为 `CLOCK_READTIME`
/// - `tp` 指向要设置的用户指针
pub fn sys_clock_gettime(_clock_id: usize, ts: *mut TimeSpec) -> Result {
    // TODO: 目前只考虑挂钟时间
    assert_eq!(_clock_id, CLOCK_REALTIME);
    let ts = unsafe { check_ptr_mut(ts)? };
    let us = get_time_ns();
    ts.sec = us / NANO_PER_SEC;
    ts.nsec = us % NANO_PER_SEC;
    Ok(0)
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: ThreadStatus,
    pub time: usize,
}

pub fn sys_setpriority(_prio: isize) -> Result {
    todo!()
}

#[repr(C)]
pub struct Tms {
    tms_utime: usize,
    tms_stime: usize,
    tms_cutime: usize,
    tms_cstime: usize,
}

/// FIXME: sys_times 暂时是非正确的实现
pub fn sys_times(tms: *mut Tms) -> Result {
    let ticks = riscv::register::time::read();
    let tms = unsafe { check_ptr_mut(tms)? };
    tms.tms_utime = ticks / 4;
    tms.tms_stime = ticks / 4;
    tms.tms_cutime = ticks / 4;
    tms.tms_cstime = ticks / 4;
    Ok(0)
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

    if VirtAddr(addr).page_offset() != 0 || len == 0 {
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
        let process = curr_process();
        log::debug!("pid: {}", process.pid());
        let mut inner = process.inner();
        // TODO: 还没有处理 MmapFlags::MAP_FIXED 的情况？
        return inner.memory_set.try_map(
            VirtAddr(addr).vpn()..VirtAddr(addr + len).vpn(),
            prot.into(),
            false,
        );
    }

    // FIXME: "其他映射尚未实现"
    Err(code::UNSUPPORTED)
}

pub fn sys_munmap(_addr: usize, _len: usize) -> Result {
    Err(code::UNSUPPORTED)
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
    let thread = curr_task().unwrap();
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
    let process = curr_process();
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
    let process = curr_process();
    let mut inner = process.inner();

    if !old_act.is_null() {
        let old_act = unsafe { check_ptr_mut(old_act)? };
        *old_act = inner.sig_handlers.action(signal);
    }

    if !act.is_null() {
        let act = unsafe { check_ptr(act)? };
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
    let thread = curr_task().unwrap();
    let mut inner = thread.inner();

    let sig_set = &mut inner.sig_receiver.mask;
    if !old_set.is_null() {
        let old_set = unsafe { check_ptr_mut(old_set)? };
        *old_set = *sig_set;
    }
    if set.is_null() {
        return Ok(0);
    }
    const SIG_BLOCK: usize = 0;
    const SIG_UNBLOCK: usize = 1;
    const SIG_SETMASK: usize = 2;

    let set = unsafe { check_ptr(set)? };
    match how {
        SIG_BLOCK => {
            sig_set.insert(*set);
        }
        SIG_UNBLOCK => {
            sig_set.remove(*set);
        }
        SIG_SETMASK => {
            *sig_set = *set;
        }
        _ => return Err(code::EINVAL),
    }

    Ok(0)
}

/// 返回系统信息，返回值为 0
pub fn sys_uname(utsname: *mut UtsName) -> Result {
    let utsname = unsafe { check_ptr_mut(utsname)? };
    *utsname = UtsName::default();
    Ok(0)
}

/// 设置进程组号
///
/// TODO: 暂时未实现，仅返回 0
pub fn sys_setpgid(_pid: usize, _pgid: usize) -> Result {
    Ok(0)
}

/// 返回进程组号
///
/// TODO: 暂时未实现，仅返回 0
pub fn sys_getpgid(_pid: usize) -> Result {
    Ok(0)
}
