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
pub(crate) const NVJPEG_STATUS_BAD_JPEG: nvjpegStatus_t = 3;

/// `libraryPropertyType`, library_types.h:158.
pub(crate) type libraryPropertyType = c_int;
pub(crate) const MAJOR_VERSION: libraryPropertyType = 0;
pub(crate) const MINOR_VERSION: libraryPropertyType = 1;
pub(crate) const PATCH_LEVEL: libraryPropertyType = 2;

/// nvjpeg.h:67. Length of the per-component arrays nvJPEG fills in.
/// `usize` rather than `c_int` because Rust array lengths are `usize`.
pub(crate) const NVJPEG_MAX_COMPONENT: usize = 4;

/// `nvjpegChromaSubsampling_t`, nvjpeg.h:109. Includes -1, one more reason
/// this can't be a Rust enum.
pub(crate) type nvjpegChromaSubsampling_t = c_int;
pub(crate) const NVJPEG_CSS_444: nvjpegChromaSubsampling_t = 0;
pub(crate) const NVJPEG_CSS_422: nvjpegChromaSubsampling_t = 1;
pub(crate) const NVJPEG_CSS_420: nvjpegChromaSubsampling_t = 2;
pub(crate) const NVJPEG_CSS_440: nvjpegChromaSubsampling_t = 3;
pub(crate) const NVJPEG_CSS_411: nvjpegChromaSubsampling_t = 4;
pub(crate) const NVJPEG_CSS_410: nvjpegChromaSubsampling_t = 5;
pub(crate) const NVJPEG_CSS_GRAY: nvjpegChromaSubsampling_t = 6;
pub(crate) const NVJPEG_CSS_410V: nvjpegChromaSubsampling_t = 7;
pub(crate) const NVJPEG_CSS_UNKNOWN: nvjpegChromaSubsampling_t = -1;

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

    /// nvjpeg.h:375. `widths` and `heights` must each point to
    /// NVJPEG_MAX_COMPONENT ints.
    pub(crate) fn nvjpegGetImageInfo(
        handle: nvjpegHandle_t,
        data: *const u8,
        length: usize,
        nComponents: *mut c_int,
        subsampling: *mut nvjpegChromaSubsampling_t,
        widths: *mut c_int,
        heights: *mut c_int,
    ) -> nvjpegStatus_t;
}
