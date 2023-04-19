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
use memory::{MemorySet, VirtAddr, VirtPageNum};
use signal::SignalHandlers;
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
        let pcb = ProcessControlBlock::new();
        Loader::load(&mut pcb.inner(), path, args)?;
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
        // 这里不需要再分配用户栈了，但需要分配内核栈，并且复制父进程的 trap context
        let thread = Arc::new(ThreadControlBlock::new(&child));
        unsafe {
            *thread.inner().trap_ctx() = parent_inner.main_thread().inner().trap_ctx().clone();
        }
        child.inner().threads.push(Some(Arc::clone(&thread)));
        // 将新线程加入调度
        add_task(thread);
        child
    }

    /// 根据 `path` 加载一个新的 ELF 文件并执行。目前必须原进程仅有一个线程
    pub fn exec(&self, path: String, args: Vec<String>) -> Result<()> {
        let mut inner = self.inner();
        assert_eq!(inner.thread_count(), 1);
        Loader::load(&mut inner, path, args)
    }

    // TODO: 这个不是正确实现的，注意
    pub fn _spawn(self: &Arc<Self>, _elf_data: &[u8]) -> isize {
        // TODO: PCB::new() 这个接口似乎可以废除，加上一个加载 elf 的接口
        let child = Self::new();
        let mut parent_inner = self.inner();
        parent_inner.children.push(Arc::clone(&child));

        let mut child_inner = child.inner();
        child_inner.fd_table = parent_inner.fd_table.clone();
        child_inner.parent = Arc::downgrade(self);

        child.pid.0 as isize
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
    pub cwd: String,

    pub sig_handlers: SignalHandlers,
}

impl ProcessControlBlockInner {
    pub fn user_token(&self) -> usize {
        self.memory_set.token()
    }

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

    pub fn thread_ref(&self, tid: usize) -> Option<&ThreadControlBlock> {
        self.threads[tid].as_deref()
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
