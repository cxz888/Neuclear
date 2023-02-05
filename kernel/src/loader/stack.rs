use alloc::{string::String, vec::Vec};

use crate::{config::PTR_SIZE, memory::PageTable, utils::timer::get_time};

use super::AT_RANDOM;

pub struct StackInit {
    pub sp: usize,
    pub pt: PageTable,
}

pub struct InfoBlock {
    pub args: Vec<String>,
    pub envs: Vec<String>,
    pub auxv: Vec<(u8, usize)>,
}

impl StackInit {
    pub fn push_str(&mut self, s: &str) -> usize {
        self.sp -= s.len() + 1;
        let mut ptr = self.sp as *mut u8;
        // TODO: 这里或许可以优化？总之目前是一个字节推一次，比较浪费
        unsafe {
            for &byte in s.as_bytes() {
                // 这里一定是栈初始化，所以用户栈没问题就是 safe 的
                *self.pt.trans_ptr_mut(ptr).unwrap() = byte;
                ptr = ptr.add(1);
            }
            // 按规范而言，这里的字符串都是符合 c 标准的字符串，末尾为 `\0`
            *self.pt.trans_ptr_mut(ptr).unwrap() = 0u8;
        }
        self.sp
    }

    pub fn push_ptrs(&mut self, ptrs: &[usize]) {
        for &ptr in ptrs.iter().rev() {
            self.push_usize(ptr)
        }
    }

    pub fn push_usize(&mut self, ptr: usize) {
        self.sp -= PTR_SIZE;
        // 用户栈不出问题则是 safe
        unsafe {
            *self.pt.trans_ptr_mut(self.sp as _).unwrap() = ptr;
        }
    }

    /// 由于用户库需要 argv 放入 a1 寄存器，这里返回一下。
    pub fn init_stack(&mut self, info_block: InfoBlock) -> usize {
        self.sp -= (info_block.args.len() + 1) * PTR_SIZE;

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
        // 清空低 3 位，也就是对齐到 8 字节
        self.sp &= !0b111;
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
