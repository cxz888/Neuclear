use super::{add_task, id::RecycleAllocator, pid_alloc, PidHandle, TaskControlBlock};
use crate::{
    error::Result,
    fs::{File, Stdin, Stdout},
    loader::Loader,
    mm::{MemorySet, VirtAddr, VirtPageNum},
    sync::{Condvar, Mutex, Semaphore, UPSafeCell},
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

pub struct ProcessControlBlockInner {
    pub is_zombie: bool,
    pub memory_set: MemorySet,
    pub parent: Weak<ProcessControlBlock>,
    pub children: Vec<Arc<ProcessControlBlock>>,
    pub exit_code: i32,
    pub heap_start: VirtPageNum,
    pub brk: usize,
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    pub task_res_allocator: RecycleAllocator,
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
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
        self.task_res_allocator.alloc()
    }

    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }

    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
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

impl ProcessControlBlock {
    pub fn inner_exclusive_access(&self) -> RefMut<ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }

    /// 一个空的进程，接下来应该紧跟着 `load()` 来加载 ELF 数据。
    pub fn new() -> Arc<Self> {
        // allocate a pid
        let pid = pid_alloc();
        let process = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
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
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    sem_list: Vec::new(),
                    condvar_list: Vec::new(),
                })
            },
        });
        // 创建主线程，这里不急着分配线程资源，因为 load 中会分配
        let task = Arc::new(TaskControlBlock::new(&process, false));
        process
            .inner_exclusive_access()
            .tasks
            .push(Some(Arc::clone(&task)));

        // 将主线程加入调度器
        add_task(task);
        process
    }

    /// Fork from parent to child
    /// Only support processes with a single thread.
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        let mut parent_inner = self.inner_exclusive_access();
        assert_eq!(parent_inner.thread_count(), 1);
        // clone parent's memory_set completely including trampoline/ustacks/trap_ctxs
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        // alloc a pid
        let pid = pid_alloc();
        // copy fd table
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent_inner.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // create child process pcb
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Arc::downgrade(self),
                    children: Vec::new(),
                    exit_code: 0,
                    heap_start: parent_inner.heap_start,
                    brk: parent_inner.brk,
                    fd_table: new_fd_table,
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    sem_list: Vec::new(),
                    condvar_list: Vec::new(),
                })
            },
        });
        // add child
        parent_inner.children.push(Arc::clone(&child));
        // create main thread of child process
        let task = Arc::new(TaskControlBlock::new(
            &child,
            // here we do not allocate trap_ctx or ustack again
            // but mention that we allocate a new kernel_stack here
            false,
        ));
        // attach task to child process
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        drop(child_inner);
        // modify kernel_stack_top in trap_ctx of this thread
        let mut task_inner = task.inner_exclusive_access();
        let trap_ctx = task_inner.trap_ctx();
        trap_ctx.kernel_sp = task.kernel_stack.top();
        drop(task_inner);
        // add this thread to scheduler
        add_task(task);
        child
    }

    /// Load a new elf to replace the original application address space and start execution
    /// Only support processes with a single thread.
    pub fn exec(&self, path: &str, args: Vec<String>) -> Result<()> {
        let mut inner = self.inner_exclusive_access();
        assert_eq!(inner.thread_count(), 1);
        // memory_set with elf program headers/trampoline/trap context/user stack
        Loader::load(&mut inner, path, args)
    }

    pub fn _spawn(self: &Arc<Self>, _elf_data: &[u8]) -> isize {
        let child = Self::new();
        // TODO: load ELF 数据
        let mut parent_inner = self.inner_exclusive_access();
        parent_inner.children.push(Arc::clone(&child));

        let mut child_inner = child.inner_exclusive_access();
        child_inner.fd_table = parent_inner.fd_table.clone();
        child_inner.parent = Arc::downgrade(self);

        child.pid.0 as isize
    }

    #[inline]
    pub fn pid(&self) -> usize {
        self.pid.0
    }
}
