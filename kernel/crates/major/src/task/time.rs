use super::{add_task, ThreadControlBlock};
use alloc::{collections::BinaryHeap, sync::Arc};
use core::cmp::Ordering;
use lazy_static::lazy_static;
use utils::{time::get_time_ms, upcell::UPSafeCell};

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

/// 返回值表示在初赛测试中是否可以继续而非等待
pub fn check_timer() -> bool {
    let current_ms = get_time_ms();
    let mut timers = TIMERS.exclusive_access();
    while let Some(timer) = timers.peek() {
        if timer.expire_ms <= current_ms {
            // 防止睡眠期间进程已经退出了
            // TODO: 但这也就意味着可能有线程是在进程退出时未被销毁的，是否要修改？
            if timer.thread.process.strong_count() > 0 {
                add_task(Arc::clone(&timer.thread));
            }
            timers.pop();
        } else {
            break;
        }
    }
    timers.is_empty()
}
