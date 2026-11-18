//! Fixed-capacity byte buffer backed by pinned host memory from a CUDA memory pool.
use cuda_core::{DriverError, MemPool, Stream, free_async, malloc_from_pool_async, sys};

use std::{ops::Deref, slice, sync::Arc};

use crate::pool::HostPool;

/// Owns a fixed-capacity buffer of pinned host memory from a CUDA memory pool.
///
/// Behaves like a `Vec<u8>` that can't grow past `capacity`.
pub(crate) struct Buffer {
    pub(crate) ptr: *mut u8,
    len: usize,
    capacity: usize,
    stream: Arc<Stream>,
    _mempool: Arc<MemPool>,
}

impl Buffer {
    pub(crate) fn new(pool: HostPool, capacity: usize) -> Result<Self, DriverError> {
        let _mempool = pool.mempool;
        let stream = pool.stream;
        // SAFETY: `pool` and `stream` are live handles held by Arcs, and both
        // belong to the same context (the caller builds them together).
        let dptr = unsafe { malloc_from_pool_async(capacity, &_mempool, &stream) }?;

        stream.synchronize()?;
        Ok(Self {
            ptr: dptr as *mut u8,
            len: 0,
            capacity,
            stream,
            _mempool,
        })
    }

    /// Returns false (and copies nothing) if `src` doesn't fit.
    #[must_use]
    pub(crate) fn extend_from_slice(&mut self, src: &[u8]) -> bool {
        if self.len + src.len() > self.capacity {
            return false;
        }
        // SAFETY: destination [len, len + src.len()) is within capacity, and `src`
        // can't overlap memory this buffer owns exclusively.
        unsafe {
            let cu_result = sys::cuMemcpyAsync(
                (self.ptr.addr() + self.len) as u64,
                src.as_ptr().addr() as u64,
                src.len(),
                self.stream.cu_stream(),
            );
            match cu_result {
                sys::cudaError_enum_CUDA_SUCCESS => {}
                _ => return false, // Err(DriverError(cu_result)),
            };
        }
        self.len += src.len();
        match self.stream.synchronize() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    pub(crate) fn truncate(&mut self, new_len: usize) {
        self.len = self.len.min(new_len);
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

impl Deref for Buffer {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.stream.synchronize().unwrap();
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: ptr came from malloc_from_pool_async on this stream and is freed only here.
        let _ = unsafe { free_async(self.ptr as sys::CUdeviceptr, &self.stream) };
    }
}

// SAFETY: the buffer has a single owner and the raw pointer is never shared,
// so moving it to another thread is fine.
unsafe impl Send for Buffer {}
