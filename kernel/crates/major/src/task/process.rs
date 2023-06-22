use super::curr_page_table;
use super::loader::Loader;
use super::{add_task, pid_alloc, resource::RecycleAllocator, PidHandle, ThreadControlBlock};
use alloc::{
    string::{String, ToString},
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::cell::RefMut;
use filesystem::{File, Stdin, Stdout};
use memory::{MemorySet, PTEFlags, VirtAddr, VirtPageNum, KERNEL_SPACE};
use signal::SignalHandlers;
use utils::error::code;
use utils::{error::Result, upcell::UPSafeCell};

pub struct ProcessControlBlock {
    pub pid: PidHandle,
    pcb_inner: UPSafeCell<ProcessControlBlockInner>,
}

impl ProcessControlBlock {
    #[track_caller]
    pub fn inner(&self) -> RefMut<ProcessControlBlockInner> {
        self.pcb_inner.exclusive_access()
    }

    /// 一个空的进程，接下来应该紧跟着 `load()` 来加载 ELF 数据。
    ///
    /// TODO: 这个接口未来可以废弃掉，顺便可以把 PCBInner 的 `Default` 实现也去掉
    pub fn new() -> Arc<Self> {
        // allocate a pid
        let pid = pid_alloc();
        let process = Arc::new(Self {
            pid,
            pcb_inner: unsafe { UPSafeCell::new(ProcessControlBlockInner::default()) },
        });
        // 创建主线程，这里不急着分配线程资源，因为 load 中会分配
        let thread = Arc::new(ThreadControlBlock::new(&process));
        process.inner().threads.push(Some(Arc::clone(&thread)));

        process
    }

    pub fn from_path(path: String, args: Vec<String>) -> Result<Arc<Self>> {
        // FIXME: 这里有个小坑，pcb 析构时，强计数归零了，然后内部的 main_thread 析构又需要 upgrade()，就会出问题
        let pcb = ProcessControlBlock::new();
        {
            let mut pcb_inner = pcb.inner();
            pcb_inner
                .memory_set
                .map_kernel_areas(&KERNEL_SPACE.exclusive_access().page_table);
            Loader::load(&mut pcb_inner, path, args)?;
        }
        Ok(pcb)
    }

    // TODO: 这个接口未来也许可以确定下来，代替 new() 之类的，看下面的 _spawn()
    #[allow(unused)]
    pub fn from_elf(name: String, args: Vec<String>, elf_data: &[u8]) -> Result<Arc<Self>> {
        let pcb = ProcessControlBlock::new();
        {
            let mut pcb_inner = pcb.inner();
            pcb_inner
                .memory_set
                .map_kernel_areas(&KERNEL_SPACE.exclusive_access().page_table);
            Loader::load_elf(&mut pcb_inner, name, args, elf_data)?;
        }
        Ok(pcb)
    }

    /// fork 一个新进程，目前仅支持只有一个主线程的进程
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        let mut parent_inner = self.inner();
        assert_eq!(parent_inner.thread_count(), 1);
        // 复制父进程的地址空间
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let pid = pid_alloc();
        // 复制文件描述符表
        let new_fd_table = parent_inner.fd_table.clone();
        // 创建子进程
        let child = Arc::new(Self {
            pid,
            pcb_inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    memory_set,
                    parent: Arc::downgrade(self),
                    heap_start: parent_inner.heap_start,
                    brk: parent_inner.brk,
                    fd_table: new_fd_table,
                    is_zombie: false,
                    children: Vec::new(),
                    threads: Vec::new(),
                    exit_code: 0,
                    cwd: parent_inner.cwd.clone(),
                    // TODO: 子进程需要继承父进程的信号处理吗？
                    sig_handlers: parent_inner.sig_handlers.clone(),
                    thread_res_allocator: parent_inner.thread_res_allocator.clone(),
                })
            },
        });
        // 新进程添入原进程的子进程表
        parent_inner.children.push(Arc::clone(&child));
        // 创建子进程的主线程
        // 这里不需要分配用户栈了，因为复制了地址空间，但需要分配内核栈，并且复制父进程的 trap context
        let thread = Arc::new(ThreadControlBlock::from_existed(
            parent_inner.threads[0].as_ref().unwrap(),
            &child,
        ));
        unsafe {
            *thread.inner().trap_ctx() = parent_inner.main_thread().inner().trap_ctx().clone();
        }
        child.inner().threads.push(Some(Arc::clone(&thread)));
        // 将新线程加入调度
        add_task(thread);
        child
    }

    /// 根据 `path` 加载一个新的 ELF 文件并执行。目前要求原进程仅有一个线程
    pub fn exec(&self, path: String, args: Vec<String>) -> Result<()> {
        let mut inner = self.inner();
        assert_eq!(inner.thread_count(), 1);
        Loader::load(&mut inner, path, args)?;
        // TODO: 这边是暂时的 Hack。
        // 因为 Loader::load() 并不改变 root_ppn，
        // 在 run_tasks() 中 memory.activate() 时，页表实际上不会刷新
        inner.memory_set.page_table.flush_tlb(None);
        Ok(())
    }

    // TODO: 这个不是正确实现的，注意
    pub fn _spawn(self: &Arc<Self>, name: String) -> Result<isize> {
        // TODO: PCB::new() 这个接口似乎可以废除，加上一个加载 elf 的接口
        let child = Self::from_path(name.clone(), vec![name])?;
        let mut parent_inner = self.inner();
        parent_inner.children.push(Arc::clone(&child));

        let mut child_inner = child.inner();
        child_inner.fd_table = parent_inner.fd_table.clone();
        child_inner.parent = Arc::downgrade(self);
        add_task(child_inner.main_thread());

        Ok(child.pid.0 as isize)
    }

    #[inline]
    pub fn pid(&self) -> usize {
        self.pid.0
    }
}

pub struct ProcessControlBlockInner {
    pub is_zombie: bool,
    pub memory_set: MemorySet,
    pub parent: Weak<ProcessControlBlock>,
    pub children: Vec<Arc<ProcessControlBlock>>,
    pub exit_code: i32,
    /// `head_start` 是进程创建好之后就不会改变的，记录堆底
    pub heap_start: VirtPageNum,
    /// `brk` 则随着系统调用扩张或收缩，记录堆顶。因此一般应满足 `brk` 地址大于 `heap_start`
    pub brk: usize,
    pub fd_table: Vec<Option<Arc<dyn File>>>,
    pub threads: Vec<Option<Arc<ThreadControlBlock>>>,
    pub thread_res_allocator: RecycleAllocator,
    /// cwd 应当永远有着 `/xxx/yyy/` 的形式（包括 `/`)
    pub cwd: String,

    pub sig_handlers: SignalHandlers,
}

impl ProcessControlBlockInner {
    pub fn alloc_fd(&mut self) -> usize {
        self.alloc_fd_from(0)
    }
    /// 分配出来的 fd 必然不小于 `min`
    pub fn alloc_fd_from(&mut self, min: usize) -> usize {
        if min > self.fd_table.len() {
            self.fd_table
                .extend(core::iter::repeat(None).take(min - self.fd_table.len()));
        }
        if let Some(fd) = (min..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }

    pub fn alloc_tid(&mut self) -> usize {
        self.thread_res_allocator.alloc()
    }

    pub fn dealloc_tid(&mut self, tid: usize) {
        self.thread_res_allocator.dealloc(tid)
    }

    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }

    pub fn main_thread(&self) -> Arc<ThreadControlBlock> {
        self.threads[0].as_ref().cloned().unwrap()
    }

    /// 设置用户堆顶。失败返回原来的 brk，成功则返回新的 brk
    pub fn set_user_brk(&mut self, new_brk: usize) -> usize {
        let new_end = VirtAddr(new_brk).vpn_ceil();
        if new_end <= self.heap_start {
            return self.brk;
        }
        // TODO: 注，这里是假定地址空间和物理内存都够用
        self.memory_set.set_user_brk(new_end, self.heap_start);
        self.brk = new_brk;
        new_brk
    }
}

impl Default for ProcessControlBlockInner {
    fn default() -> Self {
        Self {
            is_zombie: false,
            memory_set: MemorySet::new_bare(),
            parent: Weak::new(),
            children: Vec::new(),
            exit_code: 0,
            heap_start: VirtPageNum(0),
            brk: 0,
            fd_table: vec![
                // 0 -> stdin
                Some(Arc::new(Stdin)),
                // 1 -> stdout
                Some(Arc::new(Stdout)),
                // 2 -> stderr
                Some(Arc::new(Stdout)),
            ],
            threads: Vec::new(),
            thread_res_allocator: RecycleAllocator::new(),
            cwd: "/".to_string(),
            sig_handlers: SignalHandlers::new(),
        }
    }
}

/// 检查一个用户指针的可读性以及是否有 U 标记
///
/// TODO: 目前只检查了一页的有效性，如果结构体跨多页，则可能有问题
#[track_caller]
pub unsafe fn check_ptr<'a, T>(ptr: *const T) -> Result<&'a T> {
    let va = VirtAddr::from(ptr);
    let pt = curr_page_table();
    if let Some(pte) = pt.find_pte(va.vpn()) {
        if pte.flags().contains(PTEFlags::R | PTEFlags::U) {
            return Ok(unsafe { &*ptr });
        }
    }
    Err(code::EFAULT)
}

/// 检查一个指向连续切片的指针（如字符串）的可读性以及是否有 U 标志
/// 检查一个用户指针的读写性以及是否有 U 标记
///
/// TODO: 目前只检查了一页的有效性，如果结构体跨多页，则可能有问题
#[track_caller]
pub unsafe fn check_ptr_mut<'a, T>(ptr: *mut T) -> Result<&'a mut T> {
    let va = VirtAddr::from(ptr);
    let pt = curr_page_table();
    if let Some(pte) = pt.find_pte(va.vpn()) {
        if pte
            .flags()
            .contains(PTEFlags::R | PTEFlags::W | PTEFlags::U)
        {
            return Ok(unsafe { &mut *ptr });
        }
    }
    Err(code::EFAULT)
}

/// 检查一个指向连续切片的指针（如字符串）的可读性以及是否有 U 标志
///
/// TODO: 目前单纯是检查了下切片头部，未来可以根据长度计算是否跨等检查
///
/// # Safety
///
/// 需要用户保证 non-alias
#[track_caller]
pub unsafe fn check_slice<'a, T>(ptr: *const T, len: usize) -> Result<&'a [T]> {
    let va = VirtAddr::from(ptr);
    let pt = curr_page_table();
    if let Some(pte) = pt.find_pte(va.vpn()) {
        if pte.flags().contains(PTEFlags::R | PTEFlags::U) {
            return Ok(unsafe { core::slice::from_raw_parts(ptr, len) });
        }
    }
    Err(code::EFAULT)
}

/// 检查一个指向连续切片的指针（如字符串）的读写性以及是否有 U 标志
///
/// TODO: 目前单纯是检查了下切片头部，未来可以根据长度计算是否跨等检查
///
/// # Safety
///
/// 需要用户保证 non-alias
#[track_caller]
pub unsafe fn check_slice_mut<'a, T>(ptr: *mut T, len: usize) -> Result<&'a mut [T]> {
    let va = VirtAddr::from(ptr);
    let pt = curr_page_table();
    if let Some(pte) = pt.find_pte(va.vpn()) {
        if pte
            .flags()
            .contains(PTEFlags::R | PTEFlags::W | PTEFlags::U)
        {
            return Ok(unsafe { core::slice::from_raw_parts_mut(ptr, len) });
        }
    }
    Err(code::EFAULT)
}

/// 检查 null-terminated 的字符串指针（只读和 U 标志）
///
/// TODO: 目前只检查了字符串开头，未来应当根据跨页检查
///
/// # Safety
///
/// 需要用户保证 non-alias
#[track_caller]
pub unsafe fn check_cstr<'a>(ptr: *const u8) -> Result<&'a str> {
    let va = VirtAddr::from(ptr);
    let pt = curr_page_table();
    if let Some(pte) = pt.find_pte(va.vpn()) {
        if pte.flags().contains(PTEFlags::R | PTEFlags::U) {
            return Ok(unsafe { core::ffi::CStr::from_ptr(ptr as _).to_str().unwrap() });
        }
    }
    Err(code::EFAULT)
}
