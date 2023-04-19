//! Implementation of [`TrapContext`]

use riscv::register::sstatus::{self, Sstatus, SPP};

#[repr(C)]
#[derive(Clone)]
/// 该结构体保存了通用寄存器、sstatus、sepc 等
pub struct TrapContext {
    /// General-Purpose Register x0-31
    pub x: [usize; 32],
    /// sstatus
    pub sstatus: Sstatus,
    /// sepc
    pub sepc: usize,
}

impl TrapContext {
    pub fn set_sp(&mut self, sp: usize) {
        self.x[2] = sp;
    }
    pub fn app_init_context(entry: usize, sp: usize) -> Self {
        let mut sstatus = sstatus::read();
        // set CPU privilege to User after trapping back
        sstatus.set_spp(SPP::User);
        let mut ctx = Self {
            x: [0; 32],
            sstatus,
            sepc: entry,
        };
        ctx.set_sp(sp);
        ctx
    }
}
