#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(core::ffi::c_int);

impl Error {
    #[inline]
    pub fn as_isize(self) -> isize {
        self.0 as isize
    }
}

pub type Result<T = isize> = core::result::Result<T, Error>;

pub mod code {
    macro_rules! declare_err {
        ($err:tt, $code:literal, $($doc:expr),+) => {
            $(
            #[doc = $doc]
            )*
            pub const $err: super::Error = super::Error($code);
        };
    }

    declare_err!(UNSUPPORTED, -1, "暂时不支持该功能");
    // TODO: delete Err TEMP
    declare_err!(TEMP, -1, "临时错误！代码修正完毕后应删除！");
    declare_err!(EPERM, -1, "Operation not permitted.");
    declare_err!(ENOENT, -2, "No such file or directory.");
    declare_err!(ESRCH, -3, "No such process.");
    declare_err!(EINTR, -4, "Interrupted system call.");
    declare_err!(EIO, -5, "I/O error.");
    declare_err!(ENXIO, -6, "No such device or address.");
    declare_err!(ENOEXEC, -8, "Exec format error.");
    declare_err!(EBADF, -9, "Bad file number.");
    declare_err!(ECHILD, -10, "No child process");
    declare_err!(EAGAIN, -11, "Try again.");
    declare_err!(ENOMEM, -12, "Out of memory");
    declare_err!(EFAULT, -14, "Bad address.");
    declare_err!(EBUSY, -16, "Device or resource busy.");
    declare_err!(EEXIST, -17, "File exists.");
    declare_err!(ENOTDIR, -20, "Not a directory.");
    declare_err!(EISDIR, -21, "Is a directory.");
    declare_err!(EINVAL, -22, "Invalid argument.");
    declare_err!(EMFILE, -24, "Too many open files.");
    declare_err!(ESPIPE, -29, "Illegal seek.");
    declare_err!(ERANGE, -34, "Exceed range.");
}
