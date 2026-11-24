//! Safe wrapper over the hand-written nvJPEG bindings in `sys`.
use core::ffi::c_int;
use core::marker::PhantomData;
use core::ptr;

use log::warn;

use crate::error::{Error, Result};

mod sys;

/// Turns an nvJPEG status into a `Result`.
fn check(status: sys::nvjpegStatus_t) -> Result<()> {
    if status == sys::NVJPEG_STATUS_SUCCESS {
        Ok(())
    } else {
        Err(Error::NvJpeg(status))
    }
}

fn property(property: sys::libraryPropertyType) -> Result<c_int> {
    let mut value: c_int = 0;
    // SAFETY: `value` is a live, writable c_int for the duration of the call,
    // and nvjpegGetProperty only writes through the pointer before returning.
    check(unsafe { sys::nvjpegGetProperty(property, &mut value) })?;
    Ok(value)
}

/// The linked nvJPEG library version as (major, minor, patch).
pub(crate) fn version() -> Result<(c_int, c_int, c_int)> {
    Ok((
        property(sys::MAJOR_VERSION)?,
        property(sys::MINOR_VERSION)?,
        property(sys::PATCH_LEVEL)?,
    ))
}

/// An nvJPEG library handle (`nvjpegHandle_t`). Destroyed on drop.
///
/// Not `Send`: it's created and used on the decode thread only.
// Unused outside tests until decode.rs switches over to this module.
#[allow(dead_code)]
pub(crate) struct Handle {
    raw: sys::nvjpegHandle_t,
}

#[allow(dead_code)]
impl Handle {
    pub(crate) fn new() -> Result<Self> {
        let mut raw = ptr::null_mut();
        // SAFETY: `raw` is a writable out-pointer. On success nvJPEG stores a
        // valid handle in it, which `Self` then owns.
        check(unsafe { sys::nvjpegCreateSimple(&mut raw) })?;
        Ok(Self { raw })
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `raw` came from nvjpegCreateSimple and is destroyed only here.
        // Every `State` borrows this handle, so none can still be alive.
        let status = unsafe { sys::nvjpegDestroy(self.raw) };
        // Drop can't return an error, so log it.
        if let Err(e) = check(status) {
            warn!("nvjpegDestroy: {e}");
        }
    }
}

/// Per-decode state (`nvjpegJpegState_t`), created from a [`Handle`].
///
/// The `'h` lifetime ties it to the handle, so the compiler rejects dropping
/// the handle while a state still exists (the C API requires that order).
#[allow(dead_code)]
pub(crate) struct State<'h> {
    raw: sys::nvjpegJpegState_t,
    _handle: PhantomData<&'h Handle>,
}

#[allow(dead_code)]
impl<'h> State<'h> {
    pub(crate) fn new(handle: &'h Handle) -> Result<Self> {
        let mut raw = ptr::null_mut();
        // SAFETY: `handle.raw` is a live handle (borrowed for 'h), and `raw` is
        // a writable out-pointer that receives the new state on success.
        check(unsafe { sys::nvjpegJpegStateCreate(handle.raw, &mut raw) })?;
        Ok(Self {
            raw,
            _handle: PhantomData,
        })
    }
}

impl Drop for State<'_> {
    fn drop(&mut self) {
        // SAFETY: `raw` came from nvjpegJpegStateCreate and is destroyed only
        // here, while its handle is still alive (guaranteed by 'h).
        let status = unsafe { sys::nvjpegJpegStateDestroy(self.raw) };
        if let Err(e) = check(status) {
            warn!("nvjpegJpegStateDestroy: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_and_states_create_and_drop() {
        let handle = Handle::new().expect("nvjpegCreateSimple failed");
        let first = State::new(&handle).expect("nvjpegJpegStateCreate failed");
        let second = State::new(&handle).expect("nvjpegJpegStateCreate failed");
        assert!(!handle.raw.is_null());
        assert!(!first.raw.is_null() && !second.raw.is_null());
        assert_ne!(first.raw, second.raw);
        // States drop before the handle: locals drop in reverse order.
    }

    #[test]
    fn version_matches_header() {
        // NVJPEG_VER_MAJOR / NVJPEG_VER_MINOR, nvjpeg.h:70-71.
        let (major, minor, _patch) = version().expect("nvjpegGetProperty failed");
        assert_eq!((major, minor), (13, 3));
    }
}
