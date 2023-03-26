#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeSpec {
    pub sec: usize,
    pub nsec: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

use crate::config::CLOCK_FREQ;
use crate::sync::UPSafeCell;
use crate::task::{add_task, ThreadControlBlock};
use crate::utils::sbi::set_timer;
use alloc::collections::BinaryHeap;
use alloc::sync::Arc;
use core::cmp::Ordering;
use lazy_static::*;
use riscv::register::time;

pub const TICKS_PER_SEC: usize = 20;
pub const MILLI_PER_SEC: usize = 1_000;
pub const MICRO_PER_SEC: usize = 1_000_000;
pub const NANO_PER_SEC: usize = 1_000_000_000;

/// 应当是返回时钟次数。应该是从开机或者复位算起。
#[inline]
pub fn get_time() -> usize {
    // 我记得 RISC-V 似乎有规定 mtime 寄存器无论 RV32 还是 RV64 都是 64 位精度的？
    // 但既然人家的库返回 usize，这里也就返回 usize 吧
    time::read()
}

#[inline]
pub fn get_time_us() -> usize {
    time::read() * MICRO_PER_SEC / CLOCK_FREQ
}

#[inline]
pub fn get_time_ms() -> usize {
    time::read() * MILLI_PER_SEC / CLOCK_FREQ
}

#[inline]
pub fn get_time_ns() -> usize {
    time::read() * NANO_PER_SEC / CLOCK_FREQ
}

/// set the next timer interrupt
pub fn set_next_trigger() {
    set_timer(get_time() + CLOCK_FREQ / TICKS_PER_SEC);
}

pub struct TimerCondVar {
    pub expire_ms: usize,
    pub thread: Arc<ThreadControlBlock>,
}

impl PartialEq for TimerCondVar {
    fn eq(&self, other: &Self) -> bool {
        self.expire_ms == other.expire_ms
    }
}
impl Eq for TimerCondVar {}
impl PartialOrd for TimerCondVar {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let a = -(self.expire_ms as isize);
        let b = -(other.expire_ms as isize);
        Some(a.cmp(&b))
    }
}

impl Ord for TimerCondVar {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

lazy_static! {
    static ref TIMERS: UPSafeCell<BinaryHeap<TimerCondVar>> =
        unsafe { UPSafeCell::new(BinaryHeap::<TimerCondVar>::new()) };
}

pub fn add_timer(expire_ms: usize, thread: Arc<ThreadControlBlock>) {
    let mut timers = TIMERS.exclusive_access();
    timers.push(TimerCondVar { expire_ms, thread });
}

pub fn check_timer() {
    let current_ms = get_time_ms();
    let mut timers = TIMERS.exclusive_access();
    while let Some(timer) = timers.peek() {
        if timer.expire_ms <= current_ms {
            add_task(Arc::clone(&timer.thread));
            timers.pop();
        } else {
            break;
        }
    }
}
