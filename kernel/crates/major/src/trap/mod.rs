//! trap 流程，暂时，单核，仅支持用户 trap
//!
//! 1. 用户线程触发 trap
//! 2. 进入 trap.S 的 `__alltraps`
//! 3. 保存用户线程的上下文
//! 4. 进入 `__trap_handler`
//!
//! 这中间不涉及切换地址空间，但 trap 处理完后，可能发生调度，此时就有可能切换了

mod context;
mod syscall;

pub use context::TrapContext;

use crate::task::{
    __exit_curr_and_run_next, __suspend_curr_and_run_next, check_timer, curr_process, curr_trap_ctx,
};
use riscv::register::{
    mtvec::TrapMode,
    scause::{self, Exception, Interrupt, Trap},
    sie, sstatus, stval, stvec,
};
use syscall::syscall;
use utils::time::set_next_trigger;

core::arch::global_asm!(include_str!("trap.S"));

pub fn init() {
    extern "C" {
        fn __alltraps();
    }
    unsafe {
        stvec::write(__alltraps as usize, TrapMode::Direct);
    }
}

pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}

#[no_mangle]
pub fn __trap_handler() {
    let scause = scause::read();
    // 目前暂时不支持内核中断
    if let sstatus::SPP::Supervisor = sstatus::read().spp() {
        panic!("a trap {:?} from kernel!", scause.cause());
    }
    log::trace!("pid {}: pc-{:#x}", curr_process().pid.0, unsafe {
        curr_trap_ctx().sepc
    });
    log::debug!("Trap happened {:?}", scause.cause());
    let stval = stval::read();
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            // jump to next instruction anyway
            let mut cx: &mut TrapContext = unsafe { curr_trap_ctx() };
            cx.sepc += 4;
            // get system call return value
            let result = syscall(
                cx.x[17],
                [cx.x[10], cx.x[11], cx.x[12], cx.x[13], cx.x[14], cx.x[15]],
            );
            // cx is changed during sys_exec, so we have to call it again
            cx = unsafe { curr_trap_ctx() };
            cx.x[10] = result as usize;
        }
        Trap::Exception(Exception::StoreFault)
        | Trap::Exception(Exception::StorePageFault)
        | Trap::Exception(Exception::InstructionFault)
        | Trap::Exception(Exception::InstructionPageFault)
        | Trap::Exception(Exception::LoadFault)
        | Trap::Exception(Exception::LoadPageFault) => {
            unsafe {
                log::debug!("ctx: {:#x?}", curr_trap_ctx().x);
                log::error!(
                "[kernel] {:?} in application, bad addr = {:#x}, bad inst pc = {:#x}, core dumped.",
                scause.cause(),
                stval,
                curr_trap_ctx().sepc,
            );
            }
            // page fault exit code
            __exit_curr_and_run_next(-2);
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            unsafe {
                log::debug!("trap_ctx: {:#x?}", curr_trap_ctx().x);
                log::error!(
                    "[kernel] IllegalInstruction(pc={:#x}) in application, core dumped.",
                    curr_trap_ctx().sepc
                );
            }
            __exit_curr_and_run_next(-3);
        }
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_next_trigger();
            check_timer();
            __suspend_curr_and_run_next();
        }
        _ => {
            panic!(
                "Unsupported trap {:?}, stval = {:#x}!",
                scause.cause(),
                stval
            );
        }
    }
}
