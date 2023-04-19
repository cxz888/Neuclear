//! SBI console driver, for text output

use super::arch::console_putchar;
use super::upcell::UPSafeCell;
use core::fmt::{Arguments, Result, Write};

/// 绕过所有锁打印一个字符
#[inline]
fn putchar_raw(c: u8) {
    console_putchar(c as _);
}

/// 标准输出
pub struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> Result {
        for c in s.bytes() {
            if c == 127 {
                putchar_raw(8);
                putchar_raw(b' ');
                putchar_raw(8);
            } else {
                putchar_raw(c);
            }
        }
        Ok(())
    }
}

pub static STDOUT: UPSafeCell<Stdout> = unsafe { UPSafeCell::new(Stdout) };
pub static STDERR: UPSafeCell<Stdout> = unsafe { UPSafeCell::new(Stdout) };

/// 输出到 stdout
#[inline]
pub fn stdout_puts(fmt: Arguments) {
    STDOUT.exclusive_access().write_fmt(fmt).unwrap();
}

/// 输出到 stderr
#[inline]
#[allow(unused)]
pub fn stderr_puts(fmt: Arguments) {
    // 使 stdout 不要干扰 stderr 输出
    // 如果能拿到锁，说明此时没有核在输出 STDOUT，那么 STDERR 优先输出，不让其他核打断
    // 如不能，则有可能 STDOUT 已卡死了，此时也直接输出
    let _stdout = STDOUT.exclusive_access();
    STDERR.exclusive_access().write_fmt(fmt).unwrap();
}

#[inline]
pub fn print(args: Arguments) {
    stdout_puts(args);
}

#[inline]
#[allow(unused)]
pub fn error_print(args: Arguments) {
    stderr_puts(args);
}

/// 打印格式字串，无换行
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::console::print(core::format_args!($($arg)*));
    }
}

/// 打印格式字串，使用与 print 不同的 Mutex 锁
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {
        $crate::console::error_print(core::format_args!($($arg)*));
    }
}

/// 打印格式字串，有换行
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => {
        $crate::console::print(core::format_args!($($arg)*));
        $crate::println!();
    }
}

/// 打印格式字串，使用与 println 不同的 Mutex 锁
#[macro_export]
macro_rules! eprintln {
        () => ($crate::eprint!("\n"));
    ($($arg:tt)*) => {
        $crate::console::error_print(core::format_args!($($arg)*));
        $crate::eprintln!();
    }
}
