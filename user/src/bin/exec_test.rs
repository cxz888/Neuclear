#![no_std]
#![no_main]

extern crate user_lib;

const TESTS: &[&str] = &[
    "brk\0",
    "chdir\0",
    "clone\0",
    "close\0",
    "dup\0",
    "dup2\0",
    "execve\0",
    "exit\0",
    "fork\0",
    "fstat\0",
    "getcwd\0",
    "getdents\0",
    "getpid\0",
    "gettimeofday\0",
    "mkdir_\0",
    "mmap\0",
    "mount\0",
    "munmap\0",
    "open\0",
    "openat\0",
    "pipe\0",
    "read\0",
    "sleep\0",
    "test_echo\0",
    "times\0",
    "umount\0",
    "uname\0",
    "unlink\0",
    "wait\0",
    "waitpid\0",
    "write\0",
    "yield\0",
];

use user_lib::{exec, fork, waitpid};

#[no_mangle]
pub fn main() -> i32 {
    let mut xstate: i32 = Default::default();
    for &test in TESTS.iter() {
        let pid = fork();
        if pid == 0 {
            exec(&*test, &[core::ptr::null::<u8>()]);
        } else {
            let wait_pid = waitpid(pid as usize, &mut xstate);
            assert_eq!(pid, wait_pid);
        }
    }
    0
}
