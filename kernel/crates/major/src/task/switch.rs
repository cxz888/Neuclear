//! Rust wrapper around `__switch`.
//!
//! Switching to a different task's context happens here. The actual
//! implementation must not be in Rust and (essentially) has to be in assembly
//! language (Do you know why?), so this module really is just a wrapper around
//! `switch.S`.

use super::TaskContext;

core::arch::global_asm!(include_str!("switch.S"));

extern "C" {
    /// Switch to the context of `next_task_ctx_ptr`, saving the current context
    /// in `current_task_ctx_ptr`.
    pub fn __switch(current_task_ctx_ptr: *mut TaskContext, next_task_ctx_ptr: *const TaskContext);
}
