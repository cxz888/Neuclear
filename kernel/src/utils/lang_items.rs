//! The panic handler

// use crate::utils::{arch::shutdown, console::ANSICON};

use core::panic::PanicInfo;

use super::arch::shutdown;

#[panic_handler]
/// panic handler
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!(
            "[kernel] Panicked at {}:{} {}",
            // ANSICON::FgRed,
            // ANSICON::BgDefault,
            location.file(),
            location.line(),
            info.message().unwrap()
        );
    } else {
        println!(
            "[kernel] Panicked: {}",
            // ANSICON::FgRed,
            // ANSICON::BgDefault,
            info.message().unwrap()
        );
    }
    shutdown()
}
