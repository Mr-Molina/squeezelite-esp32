//! Lock-free Single-Producer Single-Consumer (SPSC) Audio Ringbuffer
//!
//! Designed for zero dynamic allocations on embedded targets. Backed by any contiguous
//! byte slice (including external PSRAM buffers).

use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingBufferError {
    BufferFull,
    BufferEmpty,
    NotEnoughData,
    NotEnoughSpace,
}

pub struct AudioRingBuffer<'a> {
    storage: &'a mut [u8],
    capacity: usize,
    read_pos: AtomicUsize,
    write_pos: AtomicUsize,
}

impl<'a> AudioRingBuffer<'a> {
    /// Creates a new audio ringbuffer over an existing byte slice.
    pub fn new(storage: &'a mut [u8]) -> Self {
        let capacity = storage.len();
        Self {
            storage,
            capacity,
            read_pos: AtomicUsize::new(0),
            write_pos: AtomicUsize::new(0),
        }
    }

    /// Returns the total capacity of the ringbuffer.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the number of bytes currently stored and available for reading.
    pub fn available_read(&self) -> usize {
        let r = self.read_pos.load(Ordering::Acquire);
        let w = self.write_pos.load(Ordering::Acquire);
        if w >= r {
            w - r
        } else {
            self.capacity - (r - w)
        }
    }

    /// Returns the number of bytes free and available for writing.
    pub fn available_write(&self) -> usize {
        // Reserve 1 slot to distinguish between empty and full states
        self.capacity - self.available_read() - 1
    }

    /// Writes data from `src` into the ringbuffer.
    /// Returns the number of bytes written.
    pub fn write(&mut self, src: &[u8]) -> Result<usize, RingBufferError> {
        let free = self.available_write();
        if free == 0 {
            return Err(RingBufferError::BufferFull);
        }

        let to_write = core::cmp::min(src.len(), free);
        let w = self.write_pos.load(Ordering::Relaxed);

        let space_to_end = self.capacity - w;
        if to_write <= space_to_end {
            self.storage[w..w + to_write].copy_from_slice(&src[..to_write]);
        } else {
            self.storage[w..self.capacity].copy_from_slice(&src[..space_to_end]);
            let remainder = to_write - space_to_end;
            self.storage[..remainder].copy_from_slice(&src[space_to_end..to_write]);
        }

        let new_w = (w + to_write) % self.capacity;
        self.write_pos.store(new_w, Ordering::Release);

        Ok(to_write)
    }

    /// Reads data from the ringbuffer into `dst`.
    /// Returns the number of bytes read.
    pub fn read(&mut self, dst: &mut [u8]) -> Result<usize, RingBufferError> {
        let avail = self.available_read();
        if avail == 0 {
            return Err(RingBufferError::BufferEmpty);
        }

        let to_read = core::cmp::min(dst.len(), avail);
        let r = self.read_pos.load(Ordering::Relaxed);

        let data_to_end = self.capacity - r;
        if to_read <= data_to_end {
            dst[..to_read].copy_from_slice(&self.storage[r..r + to_read]);
        } else {
            dst[..data_to_end].copy_from_slice(&self.storage[r..self.capacity]);
            let remainder = to_read - data_to_end;
            dst[data_to_end..to_read].copy_from_slice(&self.storage[..remainder]);
        }

        let new_r = (r + to_read) % self.capacity;
        self.read_pos.store(new_r, Ordering::Release);

        Ok(to_read)
    }

    /// Flushes all data in the ringbuffer, resetting read and write pointers.
    pub fn flush(&mut self) {
        self.read_pos.store(0, Ordering::Release);
        self.write_pos.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ringbuffer_write_read_wrap() {
        let mut storage = [0u8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);

        assert_eq!(rb.available_read(), 0);
        assert_eq!(rb.available_write(), 15);

        // Write 10 bytes
        let written = rb.write(b"0123456789").unwrap();
        assert_eq!(written, 10);
        assert_eq!(rb.available_read(), 10);

        // Read 6 bytes
        let mut out = [0u8; 6];
        let read = rb.read(&mut out).unwrap();
        assert_eq!(read, 6);
        assert_eq!(&out, b"012345");
        assert_eq!(rb.available_read(), 4);

        // Write 8 more bytes (should wrap around)
        let written2 = rb.write(b"abcdefgh").unwrap();
        assert_eq!(written2, 8);

        // Read remaining 12 bytes
        let mut out2 = [0u8; 12];
        let read2 = rb.read(&mut out2).unwrap();
        assert_eq!(read2, 12);
        assert_eq!(&out2, b"6789abcdefgh");
        assert_eq!(rb.available_read(), 0);
    }

    #[test]
    fn test_ringbuffer_empty_and_full() {
        let mut storage = [0u8; 8];
        let mut rb = AudioRingBuffer::new(&mut storage);

        let mut out = [0u8; 4];
        assert_eq!(rb.read(&mut out), Err(RingBufferError::BufferEmpty));

        // Max write is capacity - 1 = 7
        let written = rb.write(b"1234567").unwrap();
        assert_eq!(written, 7);

        assert_eq!(rb.write(b"8"), Err(RingBufferError::BufferFull));
    }
}
