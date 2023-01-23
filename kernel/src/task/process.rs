use super::{add_task, id::RecycleAllocator, pid_alloc, PidHandle, ThreadControlBlock};
use crate::{
    fs::{File, Stdin, Stdout},
    loader::Loader,
    memory::{MemorySet, VirtAddr, VirtPageNum},
    signal::SignalHandlers,
    sync::{Condvar, Mutex, Semaphore, UPSafeCell},
    utils::error::Result,
};
use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec,
    vec::Vec,
};
use core::cell::RefMut;

pub struct ProcessControlBlock {
    pub pid: PidHandle,
    inner: UPSafeCell<ProcessControlBlockInner>,
}

impl ProcessControlBlock {
    pub fn inner(&self) -> RefMut<ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }

    /// 一个空的进程，接下来应该紧跟着 `load()` 来加载 ELF 数据。
    pub fn new() -> Arc<Self> {
        // allocate a pid
        let pid = pid_alloc();
        let process = Arc::new(Self {
            pid,
            inner: unsafe { UPSafeCell::new(ProcessControlBlockInner::default()) },
        });
        // 创建主线程，这里不急着分配线程资源，因为 load 中会分配
        let thread = Arc::new(ThreadControlBlock::new(&process, false));
        process.inner().threads.push(Some(Arc::clone(&thread)));

        // 将主线程加入调度器
        add_task(thread);
        process
    }

    /// Fork from parent to child
    /// Only support processes with a single thread.
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        let mut parent_inner = self.inner();
        assert_eq!(parent_inner.thread_count(), 1);
        // clone parent's memory_set completely including trampoline/ustacks/trap_ctxs
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        // alloc a pid
        let pid = pid_alloc();
        // copy fd table
        let new_fd_table = parent_inner.fd_table.clone();
        // create child process pcb
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    memory_set,
                    parent: Arc::downgrade(self),
                    heap_start: parent_inner.heap_start,
                    brk: parent_inner.brk,
                    fd_table: new_fd_table,
                    ..Default::default()
                })
            },
        });
        // add child
        parent_inner.children.push(Arc::clone(&child));
        // create main thread of child process
        let thread = Arc::new(ThreadControlBlock::new(
            &child,
            // here we do not allocate trap_ctx or ustack again
            // but mention that we allocate a new kernel_stack here
            false,
        ));
        let mut child_inner = child.inner();
        child_inner.threads.push(Some(Arc::clone(&thread)));
        drop(child_inner);
        // modify kernel_stack_top in trap_ctx of this thread
        let mut thread_inner = thread.inner();
        let trap_ctx = thread_inner.trap_ctx();
        trap_ctx.kernel_sp = thread.kernel_stack.top();
        drop(thread_inner);
        // add this thread to scheduler
        add_task(thread);
        child
    }

    /// Load a new elf to replace the original application address space and start execution
    /// Only support processes with a single thread.
    pub fn exec(&self, path: &str, args: Vec<String>) -> Result<()> {
        let mut inner = self.inner();
        assert_eq!(inner.thread_count(), 1);
        // memory_set with elf program headers/trampoline/trap context/user stack
        Loader::load(&mut inner, path, args)
    }

    pub fn _spawn(self: &Arc<Self>, _elf_data: &[u8]) -> isize {
        let child = Self::new();
        // TODO: load ELF 数据
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
    pub heap_start: VirtPageNum,
    pub brk: usize,
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    pub thread_res_allocator: RecycleAllocator,
    pub threads: Vec<Option<Arc<ThreadControlBlock>>>,

    pub sig_handlers: SignalHandlers,

    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    pub sem_list: Vec<Option<Arc<Semaphore>>>,
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
}

impl ProcessControlBlockInner {
    pub fn user_token(&self) -> usize {
        self.memory_set.token()
    }

    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
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
            sig_handlers: SignalHandlers::new(),
            mutex_list: Vec::new(),
            sem_list: Vec::new(),
            condvar_list: Vec::new(),
        }
    }
}
