use alloc::{string::String, vec::Vec};

use crate::{
    config::PTR_SIZE,
    mm::{PageTable, VirtAddr},
};

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
        let mut ptr = self.sp;
        // TODO: 这里或许可以优化？总之目前是一个字节推一次，比较浪费
        for &byte in s.as_bytes() {
            *self.pt.trans_va_as_mut(VirtAddr(ptr)).unwrap() = byte;
            ptr += 1;
        }
        *self.pt.trans_va_as_mut(VirtAddr(ptr)).unwrap() = 0u8;
        self.sp
    }

    pub fn push_ptrs(&mut self, ptrs: &[usize]) {
        for &ptr in ptrs.into_iter().rev() {
            self.push_ptr(ptr)
        }
    }

    pub fn push_ptr(&mut self, ptr: usize) {
        self.sp -= PTR_SIZE;
        *self.pt.trans_va_as_mut(VirtAddr(self.sp)).unwrap() = ptr;
    }

    /// TODO: 由于用户库需要 argv 放入 a1 寄存器，这里返回一下。后续可以修一下
    pub fn init_stack(&mut self, info_block: InfoBlock) -> usize {
        self.sp -= (info_block.args.len() + 1) * PTR_SIZE;

        let argc = info_block.args.len();
        self.push_ptr(0);
        let envs: Vec<usize> = info_block
            .envs
            .into_iter()
            .map(|env| self.push_str(&env))
            .collect();
        self.push_ptr(0);
        let argv: Vec<usize> = info_block
            .args
            .into_iter()
            .map(|arg| self.push_str(&arg))
            .collect();
        // 清空低 3 位，也就是对齐到 8 字节
        self.sp &= !0b111;
        // 为 NULL 的 auxv（auxv 是键值对）
        self.push_ptr(0);
        self.push_ptr(0);

        // TODO: 这里暂时只有朴素的实现，以后可能要具体看看文档
        for (type_, value) in info_block.auxv {
            self.push_ptr(type_ as usize);
            self.push_ptr(value);
        }

        // 环境变量指针向量
        self.push_ptr(0);
        self.push_ptrs(&envs);

        // 参数指针向量
        self.push_ptr(0);
        self.push_ptrs(&argv);
        let argv_base = self.sp;

        // 推入 argc
        self.push_ptr(argc);
        argv_base
    }
}
