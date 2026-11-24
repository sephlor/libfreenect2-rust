//! Plain heap memory, used when built without the `cuda` feature.
use std::ops::Deref;

use crate::error;

/// Stand-in for the CUDA pool. There's nothing to manage, so it holds no state.
#[derive(Clone)]
pub(crate) struct HostPool;

impl HostPool {
    pub(crate) fn new() -> error::Result<Self> {
        Ok(Self)
    }

    pub(crate) fn alloc(&self, capacity: usize) -> error::Result<HostBuffer> {
        Ok(HostBuffer {
            data: Vec::with_capacity(capacity),
            capacity,
        })
    }
}

/// Behaves like a `Vec<u8>` that can't grow past `capacity`.
pub(crate) struct HostBuffer {
    data: Vec<u8>,
    capacity: usize,
}

impl HostBuffer {
    /// Returns false (and copies nothing) if `src` doesn't fit.
    #[must_use]
    pub(crate) fn extend_from_slice(&mut self, src: &[u8]) -> bool {
        if self.data.len() + src.len() > self.capacity {
            return false;
        }
        self.data.extend_from_slice(src);
        true
    }

    pub(crate) fn truncate(&mut self, new_len: usize) {
        self.data.truncate(new_len);
    }

    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }
}

impl Deref for HostBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.data
    }
}

impl AsRef<[u8]> for HostBuffer {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}
