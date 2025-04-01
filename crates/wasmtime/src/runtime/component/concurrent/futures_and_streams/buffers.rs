use {
    bytes::{Bytes, BytesMut},
    std::{
        mem::{self, MaybeUninit},
        ops::Deref,
        ptr, slice,
        vec::Vec,
    },
};

/// Trait representing a buffer which may be written to a `StreamWriter`.
#[doc(hidden)]
pub trait WriteBuffer<T>: Deref<Target = [T]> + Send + Sync + 'static {
    /// Number of items remaining to be read.
    fn remaining(&self) -> usize;
    /// Skip and drop the specified number of items.
    fn skip(&mut self, count: usize);
    /// Get a pointer to the next item to be read, if any.
    fn as_ptr(&self) -> *const T;
    /// Skip and forget (i.e. do _not_ drop) the specified number of items.
    fn forget(&mut self, count: usize);
}

/// Trait representing a buffer which may be used to read from a `StreamReader`.
#[doc(hidden)]
pub trait ReadBuffer<T>: Extend<T> + Send + Sync + 'static {
    /// Number of items which may be read before this buffer is full.
    fn remaining_capacity(&self) -> usize;
    /// Move (i.e. take ownership of) the specified items into this buffer.
    unsafe fn copy_from(&mut self, input: *const T, count: usize);
}

/// Container type for sending or receiving at most one element at a time.
///
/// This is functionally equivalent to `Option<T>`, plus some additional trait
/// implementations.
pub struct Single<T>(Option<T>);

impl<T> Single<T> {
    /// Create a new instance containing the specified value.
    pub fn new(value: T) -> Self {
        Self(Some(value))
    }

    /// Remove the value (if any) from this instance, leaving it empty.
    pub fn take(&mut self) -> Option<T> {
        self.0.take()
    }
}

impl<T> Default for Single<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> Deref for Single<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        if let Some(me) = &self.0 {
            unsafe { slice::from_raw_parts(me, 1) }
        } else {
            &[]
        }
    }
}

impl<T: Send + Sync + 'static> WriteBuffer<T> for Single<T> {
    fn remaining(&self) -> usize {
        if self.0.is_some() {
            1
        } else {
            0
        }
    }

    fn skip(&mut self, count: usize) {
        match count {
            0 => {}
            1 => {
                assert!(self.0.is_some());
                self.0 = None;
            }
            _ => panic!("cannot skip more than {} item(s)", self.remaining()),
        }
    }

    fn as_ptr(&self) -> *const T {
        if let Some(me) = &self.0 {
            me
        } else {
            ptr::null()
        }
    }

    fn forget(&mut self, count: usize) {
        match count {
            0 => {}
            1 => {
                assert!(self.0.is_some());
                mem::forget(self.take());
            }
            _ => panic!("cannot forget more than {} item(s)", self.remaining()),
        }
    }
}

impl<T> Extend<T> for Single<T> {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        let mut iter = iter.into_iter();
        if self.0.is_none() {
            self.0 = iter.next();
        }
        assert!(iter.next().is_none());
    }
}

impl<T: Send + Sync + 'static> ReadBuffer<T> for Single<T> {
    fn remaining_capacity(&self) -> usize {
        if self.0.is_some() {
            0
        } else {
            1
        }
    }

    unsafe fn copy_from(&mut self, input: *const T, count: usize) {
        match count {
            0 => {}
            1 => {
                assert!(self.0.is_none());
                self.0 = Some(input.read());
            }
            _ => panic!(
                "cannot take more than {} item(s)",
                self.remaining_capacity()
            ),
        }
    }
}

/// A `WriteBuffer` implementation, backed by a `Vec`.
pub struct VecBuffer<T> {
    buffer: Vec<MaybeUninit<T>>,
    offset: usize,
}

impl<T> VecBuffer<T> {
    /// Create a new instance with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            offset: 0,
        }
    }

    /// Reset the state of this buffer, removing all items and preserving its
    /// capacity.
    pub fn reset(&mut self) {
        self.skip_(self.remaining_());
        self.buffer.clear();
        self.offset = 0;
    }

    fn remaining_(&self) -> usize {
        self.buffer.len().checked_sub(self.offset).unwrap()
    }

    fn skip_(&mut self, count: usize) {
        assert!(count <= self.remaining_());
        for item in &mut self.buffer[self.offset..][..count] {
            drop(unsafe { item.as_mut_ptr().read() })
        }
        self.offset = self.offset.checked_add(count).unwrap();
    }
}

impl<T> Deref for VecBuffer<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        unsafe { mem::transmute::<&[MaybeUninit<T>], &[T]>(&self.buffer[self.offset..]) }
    }
}

impl<T: Send + Sync + 'static> WriteBuffer<T> for VecBuffer<T> {
    fn remaining(&self) -> usize {
        self.remaining_()
    }

    fn skip(&mut self, count: usize) {
        self.skip_(count)
    }

    fn as_ptr(&self) -> *const T {
        self.buffer[self.offset].as_ptr()
    }

    fn forget(&mut self, count: usize) {
        assert!(count <= self.remaining());
        self.offset = self.offset.checked_add(count).unwrap();
    }
}

impl<T> From<Vec<T>> for VecBuffer<T> {
    fn from(buffer: Vec<T>) -> Self {
        Self {
            buffer: unsafe { mem::transmute::<Vec<T>, Vec<MaybeUninit<T>>>(buffer) },
            offset: 0,
        }
    }
}

impl<T> Drop for VecBuffer<T> {
    fn drop(&mut self) {
        self.skip_(self.remaining_());
    }
}

impl<T: Send + Sync + 'static> ReadBuffer<T> for Vec<T> {
    fn remaining_capacity(&self) -> usize {
        self.capacity().checked_sub(self.len()).unwrap()
    }

    unsafe fn copy_from(&mut self, input: *const T, count: usize) {
        assert!(count <= self.remaining_capacity());
        ptr::copy(input, self.as_mut_ptr().add(self.len()), count);
        self.set_len(self.len() + count);
    }
}

/// A `WriteBuffer` implementation, backed by a `Bytes`.
pub struct BytesBuffer {
    buffer: Bytes,
    offset: usize,
}

impl From<Bytes> for BytesBuffer {
    fn from(buffer: Bytes) -> Self {
        Self { buffer, offset: 0 }
    }
}

impl Deref for BytesBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.buffer[self.offset..]
    }
}

impl WriteBuffer<u8> for BytesBuffer {
    fn remaining(&self) -> usize {
        self.buffer.len().checked_sub(self.offset).unwrap()
    }

    fn skip(&mut self, count: usize) {
        assert!(count <= self.remaining());
        self.offset = self.offset.checked_add(count).unwrap();
    }

    fn as_ptr(&self) -> *const u8 {
        unsafe { self.buffer.as_ptr().add(self.offset) }
    }

    fn forget(&mut self, count: usize) {
        self.skip(count)
    }
}

/// A `WriteBuffer` implementation, backed by a `BytesMut`.
pub struct BytesMutBuffer {
    buffer: BytesMut,
    offset: usize,
}

impl BytesMutBuffer {
    /// Convert this instance into the wrapped `BytesMut`.
    pub fn into_inner(self) -> BytesMut {
        self.buffer
    }
}

impl From<BytesMut> for BytesMutBuffer {
    fn from(buffer: BytesMut) -> Self {
        Self { buffer, offset: 0 }
    }
}

impl Deref for BytesMutBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.buffer[self.offset..]
    }
}

impl WriteBuffer<u8> for BytesMutBuffer {
    fn remaining(&self) -> usize {
        self.buffer.len().checked_sub(self.offset).unwrap()
    }

    fn skip(&mut self, count: usize) {
        assert!(count <= self.remaining());
        self.offset = self.offset.checked_add(count).unwrap();
    }

    fn as_ptr(&self) -> *const u8 {
        unsafe { self.buffer.as_ptr().add(self.offset) }
    }

    fn forget(&mut self, count: usize) {
        self.skip(count)
    }
}

impl ReadBuffer<u8> for BytesMut {
    fn remaining_capacity(&self) -> usize {
        self.capacity().checked_sub(self.len()).unwrap()
    }

    unsafe fn copy_from(&mut self, input: *const u8, count: usize) {
        assert!(count <= self.remaining_capacity());
        ptr::copy(input, self.as_mut_ptr().add(self.len()), count);
        self.set_len(self.len() + count);
    }
}
