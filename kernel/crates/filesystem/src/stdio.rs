use super::{File, Stat, StatMode, __suspend_current_and_run_next};
use drivers::BLOCK_SIZE;
use memory::UserBuffer;
use utils::arch::console_getchar;

/// The standard input
pub struct Stdin;
/// The standard output
pub struct Stdout;

impl File for Stdin {
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        false
    }
    fn read(&self, mut user_buf: UserBuffer) -> usize {
        assert_eq!(user_buf.len(), 1);
        // busy loop
        let mut c: usize;
        loop {
            c = console_getchar();
            if c == 0 {
                unsafe {
                    __suspend_current_and_run_next();
                }
            } else {
                break;
            }
        }
        let ch = c as u8;
        unsafe {
            user_buf.buffers[0].as_mut_ptr().write_volatile(ch);
        }
        1
    }
    fn write(&self, _user_buf: UserBuffer) -> usize {
        panic!("Cannot write to stdin!");
    }
    fn fstat(&self) -> Stat {
        Stat {
            st_dev: 1,
            st_ino: 1,
            st_nlink: 1,
            st_mode: StatMode::S_IFCHR | StatMode::S_IRWXU | StatMode::S_IRWXG | StatMode::S_IRWXO,
            st_blksize: BLOCK_SIZE,
            ..Default::default()
        }
    }
}

impl File for Stdout {
    fn readable(&self) -> bool {
        false
    }
    fn writable(&self) -> bool {
        true
    }
    fn read(&self, _user_buf: UserBuffer) -> usize {
        panic!("Cannot read from stdout!");
    }
    fn write(&self, user_buf: UserBuffer) -> usize {
        for buffer in user_buf.buffers.iter() {
            print!("{}", core::str::from_utf8(buffer).unwrap());
        }
        user_buf.len()
    }
    fn fstat(&self) -> Stat {
        Stat {
            st_dev: 1,
            st_ino: 1,
            st_nlink: 1,
            st_mode: StatMode::S_IFCHR | StatMode::S_IRWXU | StatMode::S_IRWXG | StatMode::S_IRWXO,
            st_blksize: BLOCK_SIZE,
            ..Default::default()
        }
    }
}
