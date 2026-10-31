//! Lock-free Single-Producer Single-Consumer (SPSC) Audio Ringbuffer
//!
//! Designed for zero dynamic allocations on embedded targets. Backed by any contiguous
//! byte slice (including external PSRAM buffers).
//!
//! Features:
//! - Cache-line aligned atomic pointers (`#[repr(align(64))]`) to eliminate false sharing.
//! - Single-atomic flush semantics avoiding dual-pointer race conditions.
//! - Lock-free concurrent SPSC access via interior mutability with raw pointers.
//! - Separate `Producer` and `Consumer` split handles.


use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingBufferError {
    BufferFull,
    BufferEmpty,
    NotEnoughData,
    NotEnoughSpace,
}

#[repr(align(64))]
struct CachePaddedAtomic {
    val: AtomicUsize,
}

impl CachePaddedAtomic {
    pub const fn new(val: usize) -> Self {
        Self {
            val: AtomicUsize::new(val),
        }
    }

    #[inline(always)]
    pub fn load(&self, order: Ordering) -> usize {
        self.val.load(order)
    }

    #[inline(always)]
    pub fn store(&self, val: usize, order: Ordering) {
        self.val.store(val, order);
    }
}

pub struct AudioRingBuffer<'a> {
    storage: *mut u8,
    capacity: usize,
    read_pos: CachePaddedAtomic,
    write_pos: CachePaddedAtomic,
    rewindable: AtomicUsize,
    _marker: PhantomData<&'a mut [u8]>,
}

// SAFETY: `AudioRingBuffer` owns raw pointer access to `storage` during its lifetime `'a`.
// Access to storage is synchronized via atomic load/store with acquire/release semantics
// across single-producer single-consumer threads, making it safe to send across threads.
unsafe impl<'a> Send for AudioRingBuffer<'a> {}

impl<'a> AudioRingBuffer<'a> {
    /// Creates a new audio ringbuffer over an existing byte slice.
    /// Panics if `storage.len() < 2`.
    pub fn new(storage: &'a mut [u8]) -> Self {
        assert!(storage.len() >= 2, "RingBuffer storage must be at least 2 bytes");
        let capacity = storage.len();
        Self {
            storage: storage.as_mut_ptr(),
            capacity,
            read_pos: CachePaddedAtomic::new(0),
            write_pos: CachePaddedAtomic::new(0),
            rewindable: AtomicUsize::new(0),
            _marker: PhantomData,
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
        // Reserve 1 slot to distinguish between empty and full states (CONC-007)
        self.capacity.saturating_sub(self.available_read() + 1)
    }

    /// Writes data from `src` into the ringbuffer.
    pub fn write(&mut self, src: &[u8]) -> Result<usize, RingBufferError> {
        self.write_internal(src)
    }

    /// Reads data from the ringbuffer into `dst`.
    pub fn read(&mut self, dst: &mut [u8]) -> Result<usize, RingBufferError> {
        self.read_internal(dst)
    }

    /// Peeks data from the ringbuffer without advancing the read pointer.
    pub fn peek(&self, dst: &mut [u8]) -> usize {
        let avail = self.available_read();
        if avail == 0 {
            return 0;
        }

        let to_peek = core::cmp::min(dst.len(), avail);
        let r = self.read_pos.load(Ordering::Relaxed);

        let data_to_end = self.capacity - r;
        if to_peek <= data_to_end {
            // SAFETY: `r + to_peek <= self.capacity` because `to_peek <= data_to_end` and `data_to_end = self.capacity - r`.
            // Destination pointer is valid for `to_peek` bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(self.storage.add(r), dst.as_mut_ptr(), to_peek);
            }
        } else {
            // SAFETY: The peek range wraps around buffer boundary. It is split into two disjoint contiguous slices:
            // 1. `self.storage.add(r)` of size `data_to_end` into `dst[..data_to_end]`.
            // 2. `self.storage` of size `remainder` into `dst[data_to_end..to_peek]`.
            // Both source and destination pointers are valid and non-overlapping.
            unsafe {
                core::ptr::copy_nonoverlapping(self.storage.add(r), dst.as_mut_ptr(), data_to_end);
                let remainder = to_peek - data_to_end;
                core::ptr::copy_nonoverlapping(
                    self.storage,
                    dst.as_mut_ptr().add(data_to_end),
                    remainder,
                );
            }
        }
        to_peek
    }

    /// Advances the read pointer by `count` bytes (e.g. after peeking and consuming).
    pub fn consume(&mut self, count: usize) {
        self.consume_internal(count);
    }

    /// Rewinds the read pointer by `count` bytes (e.g. when unconsumed bytes remain).
    pub fn rewind_read(&mut self, count: usize) {
        self.rewind_read_internal(count);
    }

    /// Flushes all data in the ringbuffer in a single atomic instruction (CONC-002).
    /// Advances `read_pos` to match `write_pos`, rendering the buffer empty immediately.
    pub fn flush(&mut self) {
        self.flush_internal();
    }

    /// Discards the newest unread bytes so that only the oldest `keep` unread bytes remain:
    /// the write position moves back to `keep` bytes past the read position. Returns the
    /// number of bytes discarded; `keep >= available_read()` is a no-op.
    ///
    /// This is C squeezelite's `outputbuf->writep = output.track_start` (`output.c:466`): drop
    /// what was written after a track boundary that playback has not reached.
    ///
    /// It takes `&mut self`, so it cannot run while [`Self::split`] handles exist. A concurrent
    /// consumer may already have loaded the old write position and be copying out of the
    /// region this frees, which the producer would then overwrite, so the operation is not
    /// offered on [`Producer`]. With exclusive access it only shortens the unread region:
    /// - the read position, and the already-read bytes behind it that
    ///   [`Self::rewind_read`] may restore, are untouched;
    /// - the new write position lies inside the old unread region, so every byte still
    ///   unread was written before and stays readable in order.
    pub fn truncate_unread(&mut self, keep: usize) -> usize {
        let avail = self.available_read();
        if keep >= avail {
            return 0;
        }
        let r = self.read_pos.load(Ordering::Acquire);
        let new_w = (r + keep) % self.capacity;
        self.write_pos.store(new_w, Ordering::Release);
        avail - keep
    }

    /// Splits the ringbuffer into disjoint Producer and Consumer handles (CONC-001).
    pub fn split(&mut self) -> (Producer<'_, 'a>, Consumer<'_, 'a>) {
        (
            Producer {
                ring: self,
                _not_sync: PhantomData,
            },
            Consumer {
                ring: self,
                _not_sync: PhantomData,
            },
        )
    }

    pub(crate) fn write_internal(&self, src: &[u8]) -> Result<usize, RingBufferError> {
        let avail_read = self.available_read();
        let free = self.capacity.saturating_sub(avail_read + 1);
        if free == 0 {
            return Err(RingBufferError::BufferFull);
        }

        let to_write = core::cmp::min(src.len(), free);
        let max_rew = self.capacity.saturating_sub(avail_read + to_write);
        self.rewindable.fetch_min(max_rew, Ordering::Release);

        let w = self.write_pos.load(Ordering::Relaxed);

        let space_to_end = self.capacity - w;
        if to_write <= space_to_end {
            // SAFETY: `w + to_write <= self.capacity` because `to_write <= space_to_end` and `space_to_end = self.capacity - w`.
            // `src` is valid for `to_write` bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(src.as_ptr(), self.storage.add(w), to_write);
            }
        } else {
            // SAFETY: The write range wraps around buffer boundary. It is split into two disjoint contiguous slices:
            // 1. `src[..space_to_end]` into `self.storage.add(w)`.
            // 2. `src[space_to_end..to_write]` into `self.storage`.
            // Both source and destination pointers are valid and non-overlapping.
            unsafe {
                core::ptr::copy_nonoverlapping(src.as_ptr(), self.storage.add(w), space_to_end);
                let remainder = to_write - space_to_end;
                core::ptr::copy_nonoverlapping(
                    src.as_ptr().add(space_to_end),
                    self.storage,
                    remainder,
                );
            }
        }

        let new_w = (w + to_write) % self.capacity;
        self.write_pos.store(new_w, Ordering::Release);

        Ok(to_write)
    }

    #[allow(deprecated)]
    pub(crate) fn read_internal(&self, dst: &mut [u8]) -> Result<usize, RingBufferError> {
        let avail = self.available_read();
        if avail == 0 {
            return Err(RingBufferError::BufferEmpty);
        }

        let to_read = core::cmp::min(dst.len(), avail);
        let r = self.read_pos.load(Ordering::Relaxed);

        let data_to_end = self.capacity - r;
        if to_read <= data_to_end {
            // SAFETY: `r + to_read <= self.capacity` because `to_read <= data_to_end` and `data_to_end = self.capacity - r`.
            // `dst` is valid for `to_read` bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(self.storage.add(r), dst.as_mut_ptr(), to_read);
            }
        } else {
            // SAFETY: The read range wraps around buffer boundary. It is split into two disjoint contiguous slices:
            // 1. `self.storage.add(r)` of size `data_to_end` into `dst[..data_to_end]`.
            // 2. `self.storage` of size `remainder` into `dst[data_to_end..to_read]`.
            // Both source and destination pointers are valid and non-overlapping.
            unsafe {
                core::ptr::copy_nonoverlapping(self.storage.add(r), dst.as_mut_ptr(), data_to_end);
                let remainder = to_read - data_to_end;
                core::ptr::copy_nonoverlapping(
                    self.storage,
                    dst.as_mut_ptr().add(data_to_end),
                    remainder,
                );
            }
        }

        let new_r = (r + to_read) % self.capacity;
        self.read_pos.store(new_r, Ordering::Release);

        let _ = self.rewindable.fetch_update(Ordering::Release, Ordering::Relaxed, |cur| {
            Some(cur.saturating_add(to_read).min(self.capacity()))
        });

        Ok(to_read)
    }

    #[allow(deprecated)]
    pub(crate) fn consume_internal(&self, count: usize) {
        let avail = self.available_read();
        let to_consume = core::cmp::min(count, avail);
        let r = self.read_pos.load(Ordering::Relaxed);
        let new_r = (r + to_consume) % self.capacity;
        self.read_pos.store(new_r, Ordering::Release);

        let _ = self.rewindable.fetch_update(Ordering::Release, Ordering::Relaxed, |cur| {
            Some(cur.saturating_add(to_consume).min(self.capacity()))
        });
    }

    #[allow(deprecated)]
    pub(crate) fn rewind_read_internal(&self, count: usize) {
        let rew = self.rewindable.load(Ordering::Acquire);
        let to_rewind = count.min(rew).min(self.available_write());
        let r = self.read_pos.load(Ordering::Relaxed);
        let new_r = if r >= to_rewind {
            r - to_rewind
        } else {
            self.capacity - (to_rewind - r)
        };
        self.read_pos.store(new_r, Ordering::Release);
        let _ = self.rewindable.fetch_update(Ordering::AcqRel, Ordering::Relaxed, |cur| {
            Some(cur.saturating_sub(to_rewind))
        });
    }

    pub(crate) fn flush_internal(&self) {
        let w = self.write_pos.load(Ordering::Acquire);
        self.read_pos.store(w, Ordering::Release);
        self.rewindable.store(0, Ordering::Release);
    }
}

/// Exclusive SPSC producer handle. Explicitly `!Sync` and `!Clone` so only a single
/// thread can ever invoke `write` (AUDIO-707).
pub struct Producer<'ring, 'buf> {
    ring: &'ring AudioRingBuffer<'buf>,
    _not_sync: PhantomData<core::cell::UnsafeCell<()>>,
}

// SAFETY: `Producer` holds an immutable reference to the ringbuffer and has exclusive write
// access via SPSC protocol. It contains `!Sync` PhantomData so it cannot be shared across threads simultaneously.
unsafe impl<'ring, 'buf> Send for Producer<'ring, 'buf> {}

impl<'ring, 'buf> Producer<'ring, 'buf> {
    pub fn write(&self, src: &[u8]) -> Result<usize, RingBufferError> {
        self.ring.write_internal(src)
    }

    pub fn available_write(&self) -> usize {
        self.ring.available_write()
    }
}

/// Exclusive SPSC consumer handle. Explicitly `!Sync` and `!Clone` so only a single
/// thread can ever invoke `read`/`consume`/`rewind_read`/`flush` (AUDIO-707).
pub struct Consumer<'ring, 'buf> {
    ring: &'ring AudioRingBuffer<'buf>,
    _not_sync: PhantomData<core::cell::UnsafeCell<()>>,
}

// SAFETY: `Consumer` holds an immutable reference to the ringbuffer and has exclusive read/rewind
// access via SPSC protocol. It contains `!Sync` PhantomData so it cannot be shared across threads simultaneously.
unsafe impl<'ring, 'buf> Send for Consumer<'ring, 'buf> {}

impl<'ring, 'buf> Consumer<'ring, 'buf> {
    pub fn read(&self, dst: &mut [u8]) -> Result<usize, RingBufferError> {
        self.ring.read_internal(dst)
    }

    pub fn available_read(&self) -> usize {
        self.ring.available_read()
    }

    pub fn peek(&self, dst: &mut [u8]) -> usize {
        self.ring.peek(dst)
    }

    pub fn consume(&self, count: usize) {
        self.ring.consume_internal(count);
    }

    pub fn rewind_read(&self, count: usize) {
        self.ring.rewind_read_internal(count);
    }

    pub fn flush(&self) {
        self.ring.flush_internal();
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

    #[test]
    fn test_ringbuffer_atomic_flush() {
        let mut storage = [0u8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);

        rb.write(b"01234567").unwrap();
        assert_eq!(rb.available_read(), 8);

        rb.flush();
        assert_eq!(rb.available_read(), 0);
        assert_eq!(rb.available_write(), 15);
    }

    #[test]
    fn test_ringbuffer_split_producer_consumer() {
        let mut storage = [0u8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);
        let (producer, consumer) = rb.split();

        assert_eq!(producer.available_write(), 15);
        assert_eq!(consumer.available_read(), 0);

        producer.write(b"HELLO").unwrap();
        assert_eq!(consumer.available_read(), 5);

        let mut buf = [0u8; 5];
        consumer.read(&mut buf).unwrap();
        assert_eq!(&buf, b"HELLO");
    }

    #[test]
    fn test_ringbuffer_consume_overflow_clamping() {
        let mut storage = [0u8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);

        rb.write(b"12345").unwrap();
        assert_eq!(rb.available_read(), 5);

        // Attempt to consume more than available
        rb.consume(100);
        assert_eq!(rb.available_read(), 0);
        assert_eq!(rb.available_write(), 15);

        // Attempt to consume when already empty
        rb.consume(50);
        assert_eq!(rb.available_read(), 0);
    }

    #[test]
    fn test_ringbuffer_rewind_read_underflow_protection() {
        let mut storage = [0xAAu8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);

        // Rewinding before any read must be a no-op (never rewinds into uninitialized storage)
        rb.rewind_read(10);
        assert_eq!(rb.available_read(), 0);

        rb.write(b"0123456789").unwrap();
        assert_eq!(rb.available_read(), 10);
        assert_eq!(rb.available_write(), 5);

        // Still 0 bytes read, so rewind must remain a no-op
        rb.rewind_read(5);
        assert_eq!(rb.available_read(), 10);

        let mut out = [0u8; 6];
        rb.read(&mut out).unwrap();
        assert_eq!(&out, b"012345");
        assert_eq!(rb.available_read(), 4);
        assert_eq!(rb.available_write(), 11);

        // Rewinding by 3 bytes should succeed normally (3 of 6 read bytes rewound)
        rb.rewind_read(3);
        assert_eq!(rb.available_read(), 7);
        assert_eq!(rb.available_write(), 8);

        // Rewinding by an excessive number of bytes must clamp to the remaining 3 read bytes,
        // NOT to available_write (8), preventing exposure of the 5 uninitialized bytes!
        rb.rewind_read(usize::MAX);
        assert_eq!(rb.available_read(), 10);
        assert_eq!(rb.available_write(), 5);

        // Verify the rewound bytes match the exact original 10 written bytes
        let mut verify = [0u8; 10];
        rb.read(&mut verify).unwrap();
        assert_eq!(&verify, b"0123456789");
    }

    #[test]
    fn test_consumer_peek_consume_flush() {
        let mut storage = [0u8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);
        let (producer, consumer) = rb.split();

        producer.write(b"ABCDEFGH").unwrap();
        assert_eq!(consumer.available_read(), 8);

        let mut peek_buf = [0u8; 4];
        let peeked = consumer.peek(&mut peek_buf);
        assert_eq!(peeked, 4);
        assert_eq!(&peek_buf, b"ABCD");
        assert_eq!(consumer.available_read(), 8); // peek doesn't advance

        consumer.consume(4);
        assert_eq!(consumer.available_read(), 4);

        let mut read_buf = [0u8; 4];
        let read = consumer.read(&mut read_buf).unwrap();
        assert_eq!(read, 4);
        assert_eq!(&read_buf, b"EFGH");
        assert_eq!(consumer.available_read(), 0);

        producer.write(b"IJKL").unwrap();
        assert_eq!(consumer.available_read(), 4);
        consumer.flush();
        assert_eq!(consumer.available_read(), 0);
    }

    fn read_all(rb: &mut AudioRingBuffer<'_>) -> std::vec::Vec<u8> {
        let mut out = std::vec![0u8; rb.available_read()];
        if !out.is_empty() {
            rb.read(&mut out).unwrap();
        }
        out
    }

    extern crate std;

    #[test]
    fn test_truncate_unread_keeps_oldest_bytes_and_writes_continue_after_them() {
        let mut storage = [0u8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);
        rb.write(b"currentNEXT").unwrap();
        assert_eq!(rb.truncate_unread(7), 4);
        assert_eq!(rb.available_read(), 7);
        assert_eq!(rb.available_write(), 8);
        // The next write lands right after the kept bytes.
        rb.write(b"more").unwrap();
        assert_eq!(read_all(&mut rb), b"currentmore");
    }

    #[test]
    fn test_truncate_unread_no_op_when_keep_covers_all_unread() {
        let mut storage = [0u8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);
        rb.write(b"abcdef").unwrap();
        assert_eq!(rb.truncate_unread(6), 0);
        assert_eq!(rb.truncate_unread(usize::MAX), 0);
        assert_eq!(read_all(&mut rb), b"abcdef");
        // Empty ring: nothing to discard either.
        assert_eq!(rb.truncate_unread(0), 0);
        assert_eq!(rb.available_read(), 0);
    }

    #[test]
    fn test_truncate_unread_across_wrap() {
        let mut storage = [0u8; 8];
        let mut rb = AudioRingBuffer::new(&mut storage);
        rb.write(b"xxxxx").unwrap();
        let mut skip = [0u8; 5];
        rb.read(&mut skip).unwrap();
        // read = write = 5; 7 bytes wrap the end of storage.
        rb.write(b"ABCdefg").unwrap();
        assert_eq!(rb.truncate_unread(3), 4);
        assert_eq!(rb.available_read(), 3);
        rb.write(b"1234").unwrap();
        assert_eq!(read_all(&mut rb), b"ABC1234");
    }

    #[test]
    fn test_truncate_unread_preserves_rewindable_history() {
        let mut storage = [0u8; 16];
        let mut rb = AudioRingBuffer::new(&mut storage);
        rb.write(b"0123").unwrap();
        let mut out = [0u8; 4];
        rb.read(&mut out).unwrap();
        rb.write(b"next").unwrap();
        // Drop everything unread; the 4 bytes already read are still behind the read position.
        assert_eq!(rb.truncate_unread(0), 4);
        assert_eq!(rb.available_read(), 0);
        rb.rewind_read(4);
        assert_eq!(read_all(&mut rb), b"0123");
    }

    #[test]
    fn test_truncate_unread_never_exposes_unwritten_bytes_to_rewind() {
        let mut storage = [0xEEu8; 8];
        let mut rb = AudioRingBuffer::new(&mut storage);
        rb.write(b"abcdefg").unwrap();
        let mut out = [0u8; 2];
        rb.read(&mut out).unwrap();
        rb.truncate_unread(1);
        // Write into the freed region, then rewind as far as allowed: only the 2 bytes read
        // ("ab") come back, never stale or overwritten storage.
        rb.write(b"XYZ").unwrap();
        rb.rewind_read(usize::MAX);
        assert_eq!(read_all(&mut rb), b"abcXYZ");
    }
}
