//! Safe wrapper over the hand-written nvJPEG bindings in `sys`.
use core::ffi::c_int;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_header() {
        // NVJPEG_VER_MAJOR / NVJPEG_VER_MINOR, nvjpeg.h:70-71.
        let (major, minor, _patch) = version().expect("nvjpegGetProperty failed");
        assert_eq!((major, minor), (13, 3));
    }
}
