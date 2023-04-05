#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]
#![feature(step_trait)]
#![feature(assert_matches)]
#![feature(let_chains)]

use riscv::register::sstatus;

use crate::utils::{logging, time};

extern crate alloc;

#[macro_use]
mod utils;
mod config;
mod driver_impl;
mod fs;
mod loader;
mod memory;
mod signal;
mod sync;
mod syscall;
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
    println!("[kernel] Hello, world!");
    memory::init();
    // 允许在内核态下访问用户数据
    unsafe { sstatus::set_sum() };
    trap::init();
    trap::enable_timer_interrupt();
    time::set_next_trigger();
    fs::list_apps();
    task::run_tasks();
}
