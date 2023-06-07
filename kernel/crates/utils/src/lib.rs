#![no_std]
#![feature(panic_info_message)]

extern crate alloc;

#[macro_use]
pub mod console;
pub mod arch;
pub mod config;
pub mod error;
pub mod lang_items;
pub mod logging;
pub mod structs;
pub mod time;
pub mod upcell;

// FIXME: dirty trick to pass test
use core::sync::atomic::AtomicBool;
pub static SHOULD_DO: AtomicBool = AtomicBool::new(false);
