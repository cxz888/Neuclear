mod stack;

use alloc::{string::String, vec::Vec};

use crate::{
    fs::{open_file, OpenFlags},
    loader::stack::{InfoBlock, StackInit},
    mm::{MemorySet, PageTable, KERNEL_SPACE},
    task::ProcessControlBlockInner,
    trap::{trap_handler, TrapContext},
};

pub struct Loader;

#[derive(Debug)]
pub enum Error {
    PathNotExisted,
}

pub type Result<T, E = Error> = core::result::Result<T, E>;
impl Loader {
    pub fn load(
        &self,
        pcb: &mut ProcessControlBlockInner,
        path: &str,
        args: Vec<String>,
    ) -> Result<()> {
        let argc = args.len();
        log::info!("path: {path}, args: {args:?}");
        let app_inode = open_file(path, OpenFlags::RDONLY).ok_or(Error::PathNotExisted)?;
        let elf_data = app_inode.read_all();
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(&elf_data);
        let new_token = memory_set.token();
        let pt = PageTable::from_token(new_token);
        // substitute memory_set
        pcb.memory_set = memory_set;
        // then we alloc user resource for main thread again
        // since memory_set has been changed
        let task = pcb.get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        let user_res = task_inner.res.as_mut().unwrap();
        user_res.ustack_base = ustack_base;
        user_res.alloc_user_res(&mut pcb.memory_set);

        let mut stack_init = StackInit {
            sp: user_res.ustack_top(),
            pt,
        };
        let info_block = InfoBlock {
            args,
            envs: Vec::new(),
            auxv: Vec::new(),
        };
        task_inner.trap_ctx_ppn = user_res.trap_ctx_ppn(pcb);
        let argv_base = stack_init.init_stack(info_block);

        // initialize trap_cx
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            stack_init.sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kernel_stack.top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = argc;
        trap_cx.x[11] = argv_base;
        *task_inner.trap_ctx() = trap_cx;
        Ok(())
    }
}
