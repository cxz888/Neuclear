mod stack;

use alloc::{string::String, vec, vec::Vec};
use goblin::elf::Elf;

use crate::{
    config::PAGE_SIZE,
    error::{code, Result},
    fs::{open_file, OpenFlags},
    loader::stack::{InfoBlock, StackInit},
    mm::{MemorySet, PageTable, KERNEL_SPACE},
    task::ProcessControlBlockInner,
    trap::{trap_handler, TrapContext},
};

pub const AT_NULL: u8 = 0;
pub const AT_PHDR: u8 = 3;
pub const AT_PHENT: u8 = 4;
pub const AT_PHNUM: u8 = 5;
pub const AT_PAGESZ: u8 = 6;
pub const AT_BASE: u8 = 7;
pub const AT_ENTRY: u8 = 9;
pub const AT_RANDOM: u8 = 25;

pub struct Loader;

impl Loader {
    pub fn load(pcb: &mut ProcessControlBlockInner, path: &str, args: Vec<String>) -> Result<()> {
        let argc = args.len();
        log::info!("path: {path}, args: {args:?}");
        let app_inode = open_file(path, OpenFlags::RDONLY).ok_or(code::ENOENT)?;
        let elf_data = app_inode.read_all();
        let elf = Elf::parse(&elf_data).expect("should be valid elf");
        let (memory_set, init_brk, entry_point) = MemorySet::from_elf(&elf, &elf_data);
        pcb.brk = init_brk;
        let new_token = memory_set.token();
        let pt = PageTable::from_token(new_token);
        // substitute memory_set
        pcb.memory_set = memory_set;
        // then we alloc user resource for main thread again
        // since memory_set has been changed
        let task = pcb.get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        let user_res = task_inner.res.as_mut().unwrap();
        user_res.alloc_user_res(&mut pcb.memory_set);

        let mut stack_init = StackInit {
            sp: user_res.user_stack_high_addr(),
            pt,
        };
        let info_block = InfoBlock {
            args,
            envs: Vec::new(),
            auxv: vec![(AT_PAGESZ, PAGE_SIZE)],
        };
        task_inner.trap_ctx_ppn = user_res.trap_ctx_ppn(pcb);
        let argv_base = stack_init.init_stack(info_block);

        // initialize trap_ctx
        let mut trap_ctx = TrapContext::app_init_context(
            entry_point,
            stack_init.sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kernel_stack.top(),
            trap_handler as usize,
        );
        trap_ctx.x[10] = argc;
        trap_ctx.x[11] = argv_base;
        *task_inner.trap_ctx() = trap_ctx;
        Ok(())
    }
}
