use alloc::{string::String, vec::Vec};
use memory::{PageTable, VirtAddr};
use utils::{
    config::{PAGE_SIZE, PTR_SIZE},
    time::get_time,
};

use super::AT_RANDOM;

pub struct StackInit {
    /// 在用户地址空间的 sp
    pub sp: usize,
    /// 对应在内核地址空间的 sp
    pub sp_kernel_va: usize,
    pub pt: PageTable,
}

pub struct InfoBlock {
    pub args: Vec<String>,
    pub envs: Vec<String>,
    pub auxv: Vec<(u8, usize)>,
}

impl StackInit {
    /// sp 和 sp_kernel_va 向下移动，如果跨越页边界，则重新翻译 sp_kernel_va
    fn sp_down(&mut self, len: usize) {
        if self.sp % PAGE_SIZE == 0 {
            self.sp -= len;
            self.sp_kernel_va = self.pt.trans_va(VirtAddr(self.sp)).unwrap().0;
        } else {
            self.sp -= len;
            self.sp_kernel_va -= len;
        }
    }

    pub fn push_str(&mut self, s: &str) -> usize {
        // 按规范而言，这里的字符串都是符合 c 标准的字符串，末尾为 `\0`
        self.push_byte(0);
        for &byte in s.as_bytes().iter().rev() {
            // 这里一定是栈初始化，所以用户栈没问题就是 safe 的
            self.push_byte(byte);
        }
        self.sp
    }

    pub fn push_ptrs(&mut self, ptrs: &[usize]) {
        for &ptr in ptrs.iter().rev() {
            self.push_usize(ptr)
        }
    }

    pub fn push_byte(&mut self, byte: u8) {
        self.sp_down(1);
        unsafe {
            *VirtAddr(self.sp_kernel_va).as_mut() = byte;
        }
    }

    pub fn push_usize(&mut self, ptr: usize) {
        self.sp_down(PTR_SIZE);
        // 只要用户栈不出问题就是 safe 的，当然，越界了还是要触发 page fault
        unsafe {
            *VirtAddr(self.sp_kernel_va).as_mut() = ptr;
        }
    }

    /// 由于用户库需要 argv 放入 a1 寄存器，这里返回一下。
    pub fn init_stack(&mut self, info_block: InfoBlock) -> usize {
        let argc = info_block.args.len();
        self.push_usize(0);
        // 这里应放入 16 字节的随机数。目前实现依赖运行时间
        // 据 Hacker News 所说，它是 "used to construct stack canaries and function pointer encryption keys"
        // 参考 https://news.ycombinator.com/item?id=24113026
        self.push_usize(get_time());
        self.push_usize(get_time());
        let random_pos = self.sp;
        let envs: Vec<usize> = info_block
            .envs
            .into_iter()
            .map(|env| self.push_str(&env))
            .collect();
        self.push_usize(0);
        let argv: Vec<usize> = info_block
            .args
            .into_iter()
            .map(|arg| self.push_str(&arg))
            .collect();
        // 清空低 3 位，也就是对齐到 8 字节，这个过程不会越过页边界
        self.sp &= !0b111;
        self.sp_kernel_va &= !0b111;
        // AT_NULL 的 auxv（auxv 是键值对）
        self.push_usize(0);
        self.push_usize(0);

        // 辅助向量
        // 随机串的地址
        self.push_usize(AT_RANDOM as usize);
        self.push_usize(random_pos);
        // type 在低地址，而 value 在高地址
        for (type_, value) in info_block.auxv {
            self.push_usize(value);
            self.push_usize(type_ as usize);
        }

        // 环境变量指针向量
        self.push_usize(0);
        self.push_ptrs(&envs);

        // 参数指针向量
        self.push_usize(0);
        self.push_ptrs(&argv);
        let argv_base = self.sp;

        // 推入 argc
        self.push_usize(argc);
        argv_base
    }
}
