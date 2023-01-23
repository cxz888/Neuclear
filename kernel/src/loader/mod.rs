mod stack;

use alloc::{collections::BTreeMap, string::String, vec, vec::Vec};
use goblin::elf::{
    header::ET_EXEC,
    program_header,
    program_header::{PF_R, PF_W, PF_X, PT_LOAD},
    Elf,
};

use crate::{
    config::PAGE_SIZE,
    fs::{open_file, OpenFlags},
    loader::stack::{InfoBlock, StackInit},
    memory::{MapArea, MapPermission, MapType, MemorySet, PageTable, VirtAddr, KERNEL_SPACE},
    task::ProcessControlBlockInner,
    trap::{trap_handler, TrapContext},
    utils::error::{code, Result},
};

// PH 相关和 Entry 应该是用于动态链接的，交由所谓 interpreter 解析
// PH 的起始地址
pub const AT_PHDR: u8 = 3;
// PH 项的大小
pub const AT_PHENT: u8 = 4;
// PH 的数量
pub const AT_PHNUM: u8 = 5;
// PAGE_SIZE 的值
pub const AT_PAGESZ: u8 = 6;
// interpreter 的基地址
pub const AT_BASE: u8 = 7;
// 可执行文件的程序入口
pub const AT_ENTRY: u8 = 9;
// 指向 16 字节随机值的地址
pub const AT_RANDOM: u8 = 25;

pub struct Loader;

impl Loader {
    /// 根据 ELF 文件内容加载所有 section，并且映射 trampoline、TrapContext 和用户栈。
    /// 加载新的任务并分配线程资源。
    ///
    /// ELF 标准参考 <https://www.sco.com/developers/gabi/latest/ch5.pheader.html>
    /// 和 <https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-elf.adoc>
    pub fn load(pcb: &mut ProcessControlBlockInner, path: &str, args: Vec<String>) -> Result<()> {
        let argc = args.len();
        log::info!("path: {path}, args: {args:?}");

        // 读取和解析 ELF 内容
        let app_inode = open_file(path, OpenFlags::RDONLY).ok_or(code::ENOENT)?;
        let elf_data = app_inode.read_all();
        let elf = Elf::parse(&elf_data).expect("should be valid elf");

        // 地址空间要清空，当然 trampoline 也不能忘了
        pcb.memory_set = MemorySet::new_bare();
        pcb.memory_set.map_trampoline();

        // 映射 ELF 中所有段

        assert!(elf.is_64);
        log::debug!("e_flags: {:#b}", elf.header.e_flags);
        assert_eq!(elf.header.e_type, ET_EXEC);
        // 确定是 RISC-V 执行环境
        assert_eq!(elf.header.e_machine, 243);
        log::info!("entry point: {:#x}", elf.entry);

        let (elf_base, elf_end) = load_sections(&elf, &elf_data, &mut pcb.memory_set);

        // program break 紧挨在 ELF 数据之后，并在之后向高地址增长
        pcb.brk = elf_end;
        pcb.heap_start = VirtAddr(elf_end).vpn();

        // 为线程分配资源
        let thread = pcb.main_thread();
        let mut thread_inner = thread.inner();
        let user_res = thread_inner.res.as_mut().unwrap();
        user_res.alloc_user_res(&mut pcb.memory_set);
        let sp = user_res.user_stack_high_addr();
        thread_inner.trap_ctx_ppn = user_res.trap_ctx_ppn(&mut pcb.memory_set).unwrap();

        // 在用户栈上推入参数、环境变量、辅助向量等
        let new_token = pcb.memory_set.token();
        let pt = PageTable::from_token(new_token);
        let mut stack_init = StackInit { sp, pt };
        let info_block = InfoBlock {
            args,
            envs: Vec::new(),
            auxv: vec![(AT_PAGESZ, PAGE_SIZE)],
        };
        let argv_base = stack_init.init_stack(info_block);

        // 初始化 trap_ctx
        let mut trap_ctx = TrapContext::app_init_context(
            elf.entry as usize,
            stack_init.sp,
            KERNEL_SPACE.exclusive_access().token(),
            thread.kernel_stack.top(),
            trap_handler as usize,
        );
        trap_ctx.x[10] = argc;
        trap_ctx.x[11] = argv_base;
        *thread_inner.trap_ctx() = trap_ctx;
        Ok(())
    }
}

// 加载所有段，返回 ELF 数据的起始地址和结束地址。结束地址向上对齐到页边界
fn load_sections(elf: &Elf, elf_data: &[u8], memory_set: &mut MemorySet) -> (usize, usize) {
    // 加载段
    let mut elf_base = 0;
    let mut elf_end = 0;
    log::debug!("ph offset: {:#x}", elf.header.e_phoff);
    for ph in &elf.program_headers {
        log::debug!("ph_type: {:?}", program_header::pt_to_str(ph.p_type));
        log::debug!("ph range: {:#x?}", ph.vm_range());
        if ph.p_type == PT_LOAD {
            // Program header 在 ELF 中的偏移为 0，所以其地址就是 ELF 段的起始地址
            if ph.p_offset == 0 {
                log::debug!("ph va: {:#x}", ph.p_vaddr);
                elf_base = ph.p_vaddr;
            }
            let start_va = VirtAddr(ph.p_vaddr as usize);
            let start_offset = start_va.page_offset();
            let end_va = VirtAddr((ph.p_vaddr + ph.p_memsz) as usize);
            let mut map_perm = MapPermission::U;
            if ph.p_flags & PF_R != 0 {
                map_perm |= MapPermission::R;
            }
            if ph.p_flags & PF_W != 0 {
                map_perm |= MapPermission::W;
            }
            if ph.p_flags & PF_X != 0 {
                map_perm |= MapPermission::X;
            }
            let map_area = MapArea::new(
                start_va,
                end_va,
                MapType::Framed {
                    data_frames: BTreeMap::new(),
                },
                map_perm,
            );
            // FIXME: 非常见鬼。在加载 elf 时，莫名其妙导致一部分数据没有加载进去（表现为全 0）。
            // 结果是重复运行任务时，有未加载的指令。怀疑可能是缓存的问题，但暂时不知道如何解决。
            // 去掉下面这个 log 可以复现
            log::debug!("file range: {:#x?}", ph.file_range());
            elf_end = map_area.vpn_range.end.page_start().0.max(elf_end);
            memory_set.push(map_area, start_offset, Some(&elf_data[ph.file_range()]));

            log::debug!("map_perm: {:?}", map_perm);
        }
    }
    (elf_base as usize, elf_end)
}
