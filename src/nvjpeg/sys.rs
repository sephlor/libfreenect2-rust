//! Raw nvJPEG declarations, written by hand from `/usr/local/cuda/include/nvjpeg.h`.
//!
//! Names match the C header so each item can be checked against it line by
//! line. Nothing here is safe to call directly; use the wrapper in `super`.
#![allow(non_camel_case_types, non_snake_case)]

use core::ffi::c_int;

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

// SAFETY: every signature below matches its declaration in nvjpeg.h (CUDA 13.5,
// nvJPEG 13.3). Linked by build.rs.
unsafe extern "C" {
    /// nvjpeg.h:288
    pub(crate) fn nvjpegGetProperty(
        type_: libraryPropertyType,
        value: *mut c_int,
    ) -> nvjpegStatus_t;
}
