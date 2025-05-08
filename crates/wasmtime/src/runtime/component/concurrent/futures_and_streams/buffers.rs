use {
    bytes::{Bytes, BytesMut},
    std::{
        io::Cursor,
        mem::{self, MaybeUninit},
        ptr, slice,
        vec::Vec,
    },
};

/// Trait representing a buffer which may be written to a `StreamWriter`.
#[doc(hidden)]
pub trait WriteBuffer<T>: Send + Sync + 'static {
    /// Slice of items remaining to be read.
    fn remaining(&self) -> &[T];
    /// Skip and drop the specified number of items.
    fn skip(&mut self, count: usize);
    /// Skip and forget (i.e. do _not_ drop) the specified number of items.
    fn forget(&mut self, count: usize);
}

/// Trait representing a buffer which may be used to read from a `StreamReader`.
#[doc(hidden)]
pub trait ReadBuffer<T>: Send + Sync + 'static {
    /// Move the specified items into this buffer.
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I);
    /// Number of items which may be read before this buffer is full.
    fn remaining_capacity(&self) -> usize;
    /// Move (i.e. take ownership of) the specified items into this buffer.
    ///
    /// SAFETY: `input` must be a valid `*const T` array of `count` items on
    /// entry.  Those items will be invalidated on exit and must be forgotten
    /// using e.g. `mem::forget` to avoid unsoundness.
    unsafe fn copy_from(&mut self, input: *const T, count: usize);
}

pub(super) struct Extender<'a, B>(pub(super) &'a mut B);

impl<T, B: ReadBuffer<T>> Extend<T> for Extender<'_, B> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.0.extend(iter)
    }
}

impl<T: Send + Sync + 'static> WriteBuffer<T> for Option<T> {
    fn remaining(&self) -> &[T] {
        if let Some(me) = self {
            // SAFETY: This effectively transmutes a `&T` to a `&[T; 1]`, which
            // should be sound.
            unsafe { slice::from_raw_parts(me, 1) }
        } else {
            &[]
        }
    }

    fn skip(&mut self, count: usize) {
        match count {
            0 => {}
            1 => {
                assert!(self.is_some());
                *self = None;
            }
            _ => panic!("cannot skip more than {} item(s)", self.remaining().len()),
        }
    }

    fn forget(&mut self, count: usize) {
        match count {
            0 => {}
            1 => {
                assert!(self.is_some());
                mem::forget(self.take());
            }
            _ => panic!("cannot forget more than {} item(s)", self.remaining().len()),
        }
    }
}

impl<T: Send + Sync + 'static> ReadBuffer<T> for Option<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let mut iter = iter.into_iter();
        if self.is_none() {
            *self = iter.next();
        }
        assert!(iter.next().is_none());
    }

    fn remaining_capacity(&self) -> usize {
        if self.is_some() {
            0
        } else {
            1
        }
    }

    /// SAFETY: See trait docs.
    unsafe fn copy_from(&mut self, input: *const T, count: usize) {
        match count {
            0 => {}
            1 => {
                assert!(self.is_none());
                *self = Some(input.read());
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
        self.skip_(self.remaining_().len());
        self.buffer.clear();
        self.offset = 0;
    }

    fn remaining_(&self) -> &[T] {
        // SAFETY: This relies on the invariant (upheld in the other methods of
        // this type) that all the elements from `self.offset` onward are
        // initialized and valid for `self.buffer`.
        unsafe { mem::transmute::<&[MaybeUninit<T>], &[T]>(&self.buffer[self.offset..]) }
    }

    fn skip_(&mut self, count: usize) {
        assert!(count <= self.remaining_().len());
        // SAFETY: See comment in `Self::remaining_`
        for item in &mut self.buffer[self.offset..][..count] {
            drop(unsafe { item.as_mut_ptr().read() })
        }
        self.offset = self.offset.checked_add(count).unwrap();
    }
}

impl<T: Send + Sync + 'static> WriteBuffer<T> for VecBuffer<T> {
    fn remaining(&self) -> &[T] {
        self.remaining_()
    }

    fn skip(&mut self, count: usize) {
        self.skip_(count)
    }

    fn forget(&mut self, count: usize) {
        assert!(count <= self.remaining().len());
        self.offset = self.offset.checked_add(count).unwrap();
    }
}

impl<T> From<Vec<T>> for VecBuffer<T> {
    fn from(buffer: Vec<T>) -> Self {
        Self {
            // SAFETY: Transmuting from `Vec<T>` to `Vec<MaybeUninit<T>>` should
            // be sound for any `T`.
            buffer: unsafe { mem::transmute::<Vec<T>, Vec<MaybeUninit<T>>>(buffer) },
            offset: 0,
        }
    }
}

impl<T> Drop for VecBuffer<T> {
    fn drop(&mut self) {
        self.skip_(self.remaining_().len());
    }
}

impl<T: Send + Sync + 'static> ReadBuffer<T> for Vec<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        Extend::extend(self, iter)
    }

    fn remaining_capacity(&self) -> usize {
        self.capacity().checked_sub(self.len()).unwrap()
    }

    /// SAFETY: See trait docs.
    unsafe fn copy_from(&mut self, input: *const T, count: usize) {
        assert!(count <= self.remaining_capacity());
        ptr::copy(input, self.as_mut_ptr().add(self.len()), count);
        self.set_len(self.len() + count);
    }
}

impl WriteBuffer<u8> for Cursor<Bytes> {
    fn remaining(&self) -> &[u8] {
        &self.get_ref()[usize::try_from(self.position()).unwrap()..]
    }

    fn skip(&mut self, count: usize) {
        assert!(
            count <= self.remaining().len(),
            "tried to skip {count} with {} remaining",
            self.remaining().len()
        );
        self.set_position(
            self.position()
                .checked_add(u64::try_from(count).unwrap())
                .unwrap(),
        );
    }

    fn forget(&mut self, count: usize) {
        self.skip(count)
    }
}

impl WriteBuffer<u8> for Cursor<BytesMut> {
    fn remaining(&self) -> &[u8] {
        &self.get_ref()[usize::try_from(self.position()).unwrap()..]
    }

    fn skip(&mut self, count: usize) {
        assert!(count <= self.remaining().len());
        self.set_position(
            self.position()
                .checked_add(u64::try_from(count).unwrap())
                .unwrap(),
        );
    }

    fn forget(&mut self, count: usize) {
        self.skip(count)
    }
}

impl ReadBuffer<u8> for BytesMut {
    fn extend<I: IntoIterator<Item = u8>>(&mut self, iter: I) {
        Extend::extend(self, iter)
    }

    fn remaining_capacity(&self) -> usize {
        self.capacity().checked_sub(self.len()).unwrap()
    }

    /// SAFETY: See trait docs.
    unsafe fn copy_from(&mut self, input: *const u8, count: usize) {
        assert!(count <= self.remaining_capacity());
        ptr::copy(input, self.as_mut_ptr().add(self.len()), count);
        self.set_len(self.len() + count);
    }
}
