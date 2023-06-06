use super::{File, Stat, StatMode, __suspend_curr_and_run_next};
use alloc::sync::{Arc, Weak};
use drivers::BLOCK_SIZE;
use utils::upcell::UPSafeCell;

const RING_BUFFER_SIZE: usize = 32;

pub struct Pipe {
    readable: bool,
    writable: bool,
    /// 可以看做这个才是管道的实体。多个 Pipe 结构体都可能持有它，且只有 Pipe 结构体会持有它。
    ///
    /// 当所有读写端都关闭时，这个管道才实际被回收
    buffer: Arc<UPSafeCell<PipeRingBuffer>>,
}
impl Pipe {
    /// Create the read end of a pipe from a ring buffer
    #[allow(unused)]
    pub fn read_end_with_buffer(buffer: Arc<UPSafeCell<PipeRingBuffer>>) -> Self {
        Self {
            readable: true,
            writable: false,
            buffer,
        }
    }
    /// Create the write end of a pipe with a ring buffer
    #[allow(unused)]
    pub fn write_end_with_buffer(buffer: Arc<UPSafeCell<PipeRingBuffer>>) -> Self {
        Self {
            readable: false,
            writable: true,
            buffer,
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
enum RingBufferStatus {
    Full,
    Empty,
    Normal,
}

/// The underlying ring buffer of a pipe
pub struct PipeRingBuffer {
    arr: [u8; RING_BUFFER_SIZE],
    head: usize,
    tail: usize,
    status: RingBufferStatus,
    write_end: Weak<Pipe>,
}

impl PipeRingBuffer {
    #[allow(unused)]
    pub fn new() -> Self {
        Self {
            arr: [0; RING_BUFFER_SIZE],
            head: 0,
            tail: 0,
            status: RingBufferStatus::Empty,
            write_end: Weak::new(),
        }
    }
    #[allow(unused)]
    pub fn set_write_end(&mut self, write_end: &Arc<Pipe>) {
        self.write_end = Arc::downgrade(write_end);
    }
    /// 调用者需自行保证缓冲区不为空，通过 `available_read` 方法即可
    pub fn read_byte(&mut self) -> u8 {
        self.status = RingBufferStatus::Normal;
        let c = self.arr[self.head];
        self.head = (self.head + 1) % RING_BUFFER_SIZE;
        if self.head == self.tail {
            self.status = RingBufferStatus::Empty;
        }
        c
    }
    /// 调用者需自行保证缓冲区不满
    pub fn write_byte(&mut self, byte: u8) {
        self.status = RingBufferStatus::Normal;
        self.arr[self.tail] = byte;
        self.tail = (self.tail + 1) % RING_BUFFER_SIZE;
        if self.head == self.tail {
            self.status = RingBufferStatus::Full;
        }
    }
    pub fn available_read(&self) -> usize {
        if let RingBufferStatus::Empty = self.status {
            0
        } else if self.tail > self.head {
            self.tail - self.head
        } else {
            self.tail + RING_BUFFER_SIZE - self.head
        }
    }
    pub fn available_write(&self) -> usize {
        RING_BUFFER_SIZE - self.available_read()
    }
    pub fn all_write_ends_closed(&self) -> bool {
        self.write_end.strong_count() == 0
    }
}

/// 返回 (read_end, write_end)
#[allow(unused)]
pub fn make_pipe() -> (Arc<Pipe>, Arc<Pipe>) {
    let buffer = unsafe { Arc::new(UPSafeCell::new(PipeRingBuffer::new())) };
    let read_end = Arc::new(Pipe::read_end_with_buffer(Arc::clone(&buffer)));
    let write_end = Arc::new(Pipe::write_end_with_buffer(Arc::clone(&buffer)));
    buffer.exclusive_access().set_write_end(&write_end);
    (read_end, write_end)
}

impl File for Pipe {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
    fn read(&self, buf: &mut [u8]) -> usize {
        assert!(self.readable());
        let mut buf_iter = buf.into_iter();
        let mut read_size = 0usize;
        loop {
            let mut ring_buffer = self.buffer.exclusive_access();
            let available_read = ring_buffer.available_read();
            if available_read == 0 {
                if ring_buffer.all_write_ends_closed() {
                    return read_size;
                }
                drop(ring_buffer);
                unsafe {
                    __suspend_curr_and_run_next();
                }
                continue;
            }
            for _ in 0..available_read {
                if let Some(byte_ref) = buf_iter.next() {
                    *byte_ref = ring_buffer.read_byte();
                    read_size += 1;
                } else {
                    return read_size;
                }
            }
        }
    }
    fn write(&self, buf: &[u8]) -> usize {
        assert!(self.writable());
        let mut buf_iter = buf.into_iter();
        let mut write_size = 0usize;
        loop {
            let mut ring_buffer = self.buffer.exclusive_access();
            let loop_write = ring_buffer.available_write();
            if loop_write == 0 {
                drop(ring_buffer);
                unsafe {
                    __suspend_curr_and_run_next();
                }
                continue;
            }
            // write at most loop_write bytes
            for _ in 0..loop_write {
                if let Some(&byte_ref) = buf_iter.next() {
                    ring_buffer.write_byte(byte_ref);
                    write_size += 1;
                } else {
                    return write_size;
                }
            }
        }
    }
    fn fstat(&self) -> Stat {
        Stat {
            st_mode: StatMode::S_IFIFO | StatMode::S_IRWXU | StatMode::S_IRWXG | StatMode::S_IRWXO,
            st_size: RING_BUFFER_SIZE as u64,
            st_blksize: BLOCK_SIZE,
            ..Default::default()
        }
    }
}
