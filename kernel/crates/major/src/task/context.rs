#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct TaskContext {
    /// Ret position after task switching
    pub ra: usize,
    /// Stack pointer
    pub sp: usize,
    /// s0-11 register, callee saved
    pub s: [usize; 12],
}

impl TaskContext {
    pub fn zero_init() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 12],
        }
    }

    pub fn goto_restore(kstack_ptr: usize) -> Self {
        extern "C" {
            fn __restore();
        }
        Self {
            ra: __restore as usize,
            sp: kstack_ptr,
            s: [0; 12],
        }
    }
}
