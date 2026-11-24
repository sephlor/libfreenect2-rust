//! Raw nvJPEG declarations, written by hand from `/usr/local/cuda/include/nvjpeg.h`.
//!
//! Names match the C header so each item can be checked against it line by
//! line. Nothing here is safe to call directly; use the wrapper in `super`.
// dead_code: a binding module can declare more than the wrapper uses so far.
#![allow(non_camel_case_types, non_snake_case, dead_code)]

use core::ffi::c_int;
use core::marker::{PhantomData, PhantomPinned};

// C enums are `int`-sized on this ABI. They're aliases plus constants rather
// than Rust enums, because C may hand back a value a Rust enum doesn't list,
// and holding such a value in a Rust enum is undefined behavior.

/// `nvjpegStatus_t`, nvjpeg.h:76.
pub(crate) type nvjpegStatus_t = c_int;
pub(crate) const NVJPEG_STATUS_SUCCESS: nvjpegStatus_t = 0;

/// `libraryPropertyType`, library_types.h:158.
pub(crate) type libraryPropertyType = c_int;
pub(crate) const MAJOR_VERSION: libraryPropertyType = 0;
pub(crate) const MINOR_VERSION: libraryPropertyType = 1;
pub(crate) const PATCH_LEVEL: libraryPropertyType = 2;

// Opaque C structs: their layout is private to nvJPEG, so Rust only ever holds
// pointers to them. A zero-sized struct per type (instead of `c_void`) means
// the compiler rejects passing a state where a handle is expected. The
// PhantomData makes them !Send, !Sync and !Unpin, so nothing assumes more
// about them than C promises (the Rustonomicon's recommended pattern).

/// `struct nvjpegHandle`, nvjpeg.h:281.
#[repr(C)]
pub(crate) struct nvjpegHandle {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type nvjpegHandle_t = *mut nvjpegHandle;

/// `struct nvjpegJpegState`, nvjpeg.h:285.
#[repr(C)]
pub(crate) struct nvjpegJpegState {
    _data: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>,
}
pub(crate) type nvjpegJpegState_t = *mut nvjpegJpegState;

// SAFETY: every signature below matches its declaration in nvjpeg.h (CUDA 13.5,
// nvJPEG 13.3). Linked by build.rs.
unsafe extern "C" {
    /// nvjpeg.h:288
    pub(crate) fn nvjpegGetProperty(
        type_: libraryPropertyType,
        value: *mut c_int,
    ) -> nvjpegStatus_t;

    /// nvjpeg.h:301. Default backend, default allocators.
    pub(crate) fn nvjpegCreateSimple(handle: *mut nvjpegHandle_t) -> nvjpegStatus_t;

    /// nvjpeg.h:323
    pub(crate) fn nvjpegDestroy(handle: nvjpegHandle_t) -> nvjpegStatus_t;

    /// nvjpeg.h:358
    pub(crate) fn nvjpegJpegStateCreate(
        handle: nvjpegHandle_t,
        jpeg_handle: *mut nvjpegJpegState_t,
    ) -> nvjpegStatus_t;

    /// nvjpeg.h:362
    pub(crate) fn nvjpegJpegStateDestroy(jpeg_handle: nvjpegJpegState_t) -> nvjpegStatus_t;
}
