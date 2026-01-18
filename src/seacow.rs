use core::mem::MaybeUninit;
#[cfg(feature = "_internal_c_ffi")]
use core::slice;

#[cfg(all(not(feature = "std"), feature = "no_std"))]
use std::{boxed::Box, vec::Vec};

#[cfg(feature = "_internal_c_ffi")]
use core::ffi::c_void;

#[derive(Clone)]
pub struct SeaCow<'a, T> {
    inner: SeaCowInner<'a, T>,
}

#[cfg(feature = "_internal_c_ffi")]
unsafe impl<T: Send> Send for SeaCowInner<'_, T> {}
#[cfg(feature = "_internal_c_ffi")]
unsafe impl<T: Sync> Sync for SeaCowInner<'_, T> {}

/// Rust assumes `*const T` is never `Send`/`Sync`, but it can be.
/// This is fudge for https://github.com/rust-lang/rust/issues/93367
#[cfg(feature = "_internal_c_ffi")]
#[repr(transparent)]
#[derive(Copy, Clone)]
pub(crate) struct Pointer<T>(pub *const T);

#[cfg(feature = "_internal_c_ffi")]
#[derive(Copy, Clone)]
#[repr(transparent)]
pub(crate) struct PointerMut<T>(pub *mut T);

#[cfg(feature = "_internal_c_ffi")]
unsafe impl<T: Send + Sync> Send for Pointer<T> {}
#[cfg(feature = "_internal_c_ffi")]
unsafe impl<T: Send + Sync> Sync for Pointer<T> {}
#[cfg(feature = "_internal_c_ffi")]
unsafe impl<T: Send + Sync> Send for PointerMut<T> {}
#[cfg(feature = "_internal_c_ffi")]
unsafe impl<T: Send + Sync> Sync for PointerMut<T> {}

impl<T> SeaCow<'static, T> {
    #[inline]
    #[must_use]
    pub fn boxed(data: Box<[T]>) -> Self {
        Self {
            inner: SeaCowInner::Boxed(data),
        }
    }
}

impl<'a, T> SeaCow<'a, T> {
    #[inline]
    #[must_use]
    pub const fn borrowed(data: &'a [T]) -> Self {
        Self {
            inner: SeaCowInner::Borrowed(data),
        }
    }

    /// The pointer must be `malloc`-allocated
    #[inline]
    #[cfg(feature = "_internal_c_ffi")]
    #[must_use]
    pub unsafe fn c_owned(
        ptr: *mut T,
        len: usize,
        free_fn: unsafe extern "C" fn(*mut c_void),
    ) -> Self {
        debug_assert!(!ptr.is_null());
        debug_assert!(len > 0);

        Self {
            inner: SeaCowInner::Owned { ptr, len, free_fn },
        }
    }

    #[inline]
    #[cfg(feature = "_internal_c_ffi")]
    pub(crate) fn make_owned(&mut self, free_fn: unsafe extern "C" fn(*mut c_void)) {
        if let SeaCowInner::Borrowed(slice) = self.inner {
            self.inner = SeaCowInner::Owned {
                ptr: slice.as_ptr().cast_mut(),
                len: slice.len(),
                free_fn,
            };
        }
    }
}

impl<T: Clone> Clone for SeaCowInner<'_, T> {
    #[inline(never)]
    fn clone(&self) -> Self {
        let slice = match self {
            Self::Borrowed(data) => return Self::Borrowed(data),
            #[cfg(feature = "_internal_c_ffi")]
            Self::Owned {
                ptr,
                len,
                free_fn: _,
            } => unsafe { slice::from_raw_parts(*ptr, *len) },
            Self::Boxed(data) => &**data,
        };
        let mut v = Vec::new();
        v.try_reserve_exact(slice.len()).unwrap();
        v.extend_from_slice(slice);
        Self::Boxed(v.into_boxed_slice())
    }
}

enum SeaCowInner<'a, T> {
    #[cfg(feature = "_internal_c_ffi")]
    Owned {
        ptr: *mut T,
        len: usize,
        free_fn: unsafe extern "C" fn(*mut c_void),
    },
    Borrowed(&'a [T]),
    Boxed(Box<[T]>),
}

#[cfg(feature = "_internal_c_ffi")]
impl<T> Drop for SeaCowInner<'_, T> {
    fn drop(&mut self) {
        if let Self::Owned { ptr, free_fn, .. } = self {
            unsafe {
                (free_fn)((*ptr).cast());
            }
        }
    }
}

impl<T> SeaCow<'_, T> {
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        match &self.inner {
            #[cfg(feature = "_internal_c_ffi")]
            SeaCowInner::Owned { ptr, len, .. } => unsafe { slice::from_raw_parts(*ptr, *len) },
            SeaCowInner::Borrowed(a) => a,
            SeaCowInner::Boxed(x) => x,
        }
    }
}

/// Read-only bitmap view - returned after remapping
pub(crate) enum RowBitmap<'a, T> {
    /// Safe contiguous data (pure Rust path)
    Contiguous { data: &'a [T], width: usize },
    /// Raw pointer rows (C FFI path)
    #[cfg(feature = "_internal_c_ffi")]
    RowPointers {
        rows: &'a [Pointer<T>],
        width: usize,
    },
}

/// Mutable bitmap view - used during remapping
pub(crate) enum RowBitmapMut<'a, T> {
    /// Safe contiguous data (pure Rust path)
    Contiguous { data: &'a mut [T], width: usize },
    /// Raw pointer rows (C FFI path)
    #[cfg(feature = "_internal_c_ffi")]
    RowPointers {
        rows: MutCow<'a, [PointerMut<T>]>,
        width: usize,
    },
}

impl<T> RowBitmapMut<'_, MaybeUninit<T>> {
    /// Convert MaybeUninit bitmap to initialized bitmap
    ///
    /// # Safety
    /// All elements must have been initialized
    #[inline]
    #[allow(unsafe_code)]
    pub(crate) fn assume_init<'maybeowned>(
        &'maybeowned mut self,
    ) -> RowBitmap<'maybeowned, T> {
        match self {
            Self::Contiguous { data, width } => {
                // SAFETY: MaybeUninit<T> and T have the same layout
                // Caller guarantees all elements are initialized
                let initialized: &[T] = unsafe {
                    &*((*data) as *const [MaybeUninit<T>] as *const [T])
                };
                RowBitmap::Contiguous {
                    data: initialized,
                    width: *width,
                }
            }
            #[cfg(feature = "_internal_c_ffi")]
            Self::RowPointers { rows, width } => {
                #[allow(clippy::transmute_ptr_to_ptr)]
                RowBitmap::RowPointers {
                    width: *width,
                    rows: unsafe {
                        core::mem::transmute::<
                            &'maybeowned [PointerMut<MaybeUninit<T>>],
                            &'maybeowned [Pointer<T>],
                        >(rows.borrow_mut())
                    },
                }
            }
        }
    }
}

impl<T> RowBitmap<'_, T> {
    #[cfg(not(feature = "_internal_c_ffi"))]
    pub fn rows(&self) -> impl Iterator<Item = &[T]> {
        match self {
            Self::Contiguous { data, width } => data.chunks_exact(*width),
        }
    }

    #[cfg(feature = "_internal_c_ffi")]
    pub fn rows(&self) -> impl Iterator<Item = &[T]> {
        match self {
            Self::Contiguous { data, width } => {
                RowBitmapIter::Contiguous(data.chunks_exact(*width))
            }
            Self::RowPointers { rows, width } => {
                let width = *width;
                RowBitmapIter::RowPointers(
                    rows.iter()
                        .map(move |row| unsafe { slice::from_raw_parts(row.0, width) }),
                )
            }
        }
    }
}

#[cfg(feature = "_internal_c_ffi")]
enum RowBitmapIter<'a, T, I: Iterator<Item = &'a [T]>> {
    Contiguous(core::slice::ChunksExact<'a, T>),
    RowPointers(I),
}

#[cfg(feature = "_internal_c_ffi")]
impl<'a, T, I: Iterator<Item = &'a [T]>> Iterator for RowBitmapIter<'a, T, I> {
    type Item = &'a [T];

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Contiguous(iter) => iter.next(),
            Self::RowPointers(iter) => iter.next(),
        }
    }
}

#[cfg(feature = "_internal_c_ffi")]
enum MutCow<'a, T: ?Sized> {
    Owned(Box<T>),
    #[allow(dead_code)]
    /// This is optional, for FFI only
    Borrowed(&'a mut T),
}

#[cfg(feature = "_internal_c_ffi")]
impl<T: ?Sized> MutCow<'_, T> {
    #[must_use]
    pub fn borrow_mut(&mut self) -> &mut T {
        match self {
            Self::Owned(a) => a,
            Self::Borrowed(a) => a,
        }
    }
}

impl<'a, T: Sync + Send + Copy + 'static> RowBitmapMut<'a, T> {
    #[inline]
    #[must_use]
    pub fn new_contiguous(data: &'a mut [T], width: usize) -> Self {
        Self::Contiguous { data, width }
    }

    /// Inner pointers must be valid for `'a` too, and at least `width` large each
    #[inline]
    #[cfg(feature = "_internal_c_ffi")]
    #[must_use]
    pub unsafe fn new(rows: &'a mut [*mut T], width: usize) -> Self {
        Self::RowPointers {
            rows: MutCow::Borrowed(&mut *(rows as *mut [*mut T] as *mut [PointerMut<T>])),
            width,
        }
    }

    #[cfg(not(feature = "_internal_c_ffi"))]
    pub fn rows_mut(&mut self) -> impl Iterator<Item = &mut [T]> + Send {
        match self {
            Self::Contiguous { data, width } => data.chunks_exact_mut(*width),
        }
    }

    #[cfg(feature = "_internal_c_ffi")]
    pub fn rows_mut(&mut self) -> impl Iterator<Item = &mut [T]> + Send {
        match self {
            Self::Contiguous { data, width } => {
                RowBitmapMutIter::Contiguous(data.chunks_exact_mut(*width))
            }
            Self::RowPointers { rows, width } => {
                let width = *width;
                RowBitmapMutIter::RowPointers(
                    rows.borrow_mut()
                        .iter()
                        .map(move |row| unsafe { slice::from_raw_parts_mut(row.0, width) }),
                )
            }
        }
    }

    #[cfg(not(feature = "_internal_c_ffi"))]
    pub(crate) fn chunks(
        &mut self,
        chunk_size: usize,
    ) -> impl Iterator<Item = RowBitmapMut<'_, T>> {
        match self {
            Self::Contiguous { data, width } => {
                let row_size = *width;
                let chunk_bytes = chunk_size * row_size;
                data.chunks_mut(chunk_bytes)
                    .map(move |chunk| RowBitmapMut::Contiguous {
                        data: chunk,
                        width: row_size,
                    })
            }
        }
    }

    #[cfg(feature = "_internal_c_ffi")]
    pub(crate) fn chunks(
        &mut self,
        chunk_size: usize,
    ) -> impl Iterator<Item = RowBitmapMut<'_, T>> {
        match self {
            Self::Contiguous { data, width } => {
                let row_size = *width;
                let chunk_bytes = chunk_size * row_size;
                RowBitmapMutChunks::Contiguous {
                    iter: data.chunks_mut(chunk_bytes),
                    width: row_size,
                }
            }
            Self::RowPointers { rows, width } => RowBitmapMutChunks::RowPointers {
                iter: rows.borrow_mut().chunks_mut(chunk_size),
                width: *width,
            },
        }
    }

    #[must_use]
    pub(crate) fn len(&mut self) -> usize {
        match self {
            Self::Contiguous { data, width } => data.len() / *width,
            #[cfg(feature = "_internal_c_ffi")]
            Self::RowPointers { rows, .. } => rows.borrow_mut().len(),
        }
    }
}

#[cfg(feature = "_internal_c_ffi")]
enum RowBitmapMutIter<'a, T, I: Iterator<Item = &'a mut [T]>> {
    Contiguous(core::slice::ChunksExactMut<'a, T>),
    RowPointers(I),
}

#[cfg(feature = "_internal_c_ffi")]
impl<'a, T: Send, I: Iterator<Item = &'a mut [T]> + Send> Iterator for RowBitmapMutIter<'a, T, I> {
    type Item = &'a mut [T];

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Contiguous(iter) => iter.next(),
            Self::RowPointers(iter) => iter.next(),
        }
    }
}

// Safe: ChunksExactMut is Send when T is Send
#[cfg(feature = "_internal_c_ffi")]
unsafe impl<'a, T: Send, I: Iterator<Item = &'a mut [T]> + Send> Send
    for RowBitmapMutIter<'a, T, I>
{
}

#[cfg(feature = "_internal_c_ffi")]
enum RowBitmapMutChunks<'a, T> {
    Contiguous {
        iter: core::slice::ChunksMut<'a, T>,
        width: usize,
    },
    RowPointers {
        iter: core::slice::ChunksMut<'a, PointerMut<T>>,
        width: usize,
    },
}

#[cfg(feature = "_internal_c_ffi")]
impl<'a, T: Sync + Send + Copy + 'static> Iterator for RowBitmapMutChunks<'a, T> {
    type Item = RowBitmapMut<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Contiguous { iter, width } => iter.next().map(|chunk| RowBitmapMut::Contiguous {
                data: chunk,
                width: *width,
            }),
            Self::RowPointers { iter, width } => {
                iter.next().map(|chunk| RowBitmapMut::RowPointers {
                    width: *width,
                    rows: MutCow::Borrowed(chunk),
                })
            }
        }
    }
}
