//! Host memory for JPEG packets: pinned through CUDA when the `cuda` feature
//! is on, plain heap memory otherwise. Both backends expose the same API.
#[cfg(feature = "cuda")]
mod cuda;
#[cfg(feature = "cuda")]
pub(crate) use cuda::{HostBuffer, HostPool};

#[cfg(not(feature = "cuda"))]
mod heap;
#[cfg(not(feature = "cuda"))]
pub(crate) use heap::{HostBuffer, HostPool};
