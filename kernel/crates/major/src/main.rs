#![no_std]
#![no_main]
#![feature(let_chains)]

extern crate alloc;
#[macro_use]
extern crate utils;

use riscv::register::sstatus;
use utils::{logging, time};

mod task;
mod trap;

core::arch::global_asm!(include_str!("entry.asm"));

/// clear BSS segment
fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    unsafe {
        core::slice::from_raw_parts_mut(sbss as usize as *mut u8, ebss as usize - sbss as usize)
            .fill(0);
    }
}

#[no_mangle]
/// the rust entry-point of os
pub fn rust_main() -> ! {
    clear_bss();
    logging::init();
    memory::init();
    println!("[kernel] Hello, world!");
    // 允许在内核态下访问用户数据
    unsafe { sstatus::set_sum() };
    trap::init();
    #[cfg(not(feature = "test"))]
    {
        // 初赛测试模式下，操作系统表现为批处理系统，挨个加载挨个运行，不需要定时器中断
        trap::enable_timer_interrupt();
        time::set_next_trigger();
    }
    task::list_apps();
    task::run_tasks();
}
