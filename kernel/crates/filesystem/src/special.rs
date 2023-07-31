use core::ops::AddAssign;

use utils::upcell::UPSafeCell;

use crate::File;

pub struct Passwd {
    offset: UPSafeCell<usize>,
}

impl Passwd {
    pub fn new() -> Self {
        Self {
            offset: unsafe { UPSafeCell::new(0) },
        }
    }
}

static PASSWD: &str = "root:x:0:0:root:/root:/bin/bash";

impl File for Passwd {
    fn readable(&self) -> bool {
        true
    }

    fn writable(&self) -> bool {
        false
    }

    fn read(&self, buf: &mut [u8]) -> usize {
        let offset = *self.offset.exclusive_access();
        let len = usize::min(PASSWD.len() - offset, buf.len());
        buf[..len].copy_from_slice(PASSWD[offset..offset + len].as_bytes());
        self.offset.exclusive_access().add_assign(len);
        return len;
    }

    fn write(&self, _buf: &[u8]) -> usize {
        panic!("Should not write passwd");
    }

    fn remove(&self, _name: &str) {
        todo!()
    }

    fn fstat(&self) -> crate::Stat {
        todo!()
    }
}
