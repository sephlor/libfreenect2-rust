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

/// Header info for one JPEG, read without decoding it.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageInfo {
    /// Size of the full-resolution (luma) plane, in pixels.
    pub(crate) width: usize,
    pub(crate) height: usize,
    /// 1 (grayscale) or 3; nvJPEG supports nothing else.
    pub(crate) components: usize,
    /// One of the `NVJPEG_CSS_*` constants.
    pub(crate) subsampling: c_int,
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

    /// Reads the JPEG header for its size and layout. Doesn't touch the GPU.
    pub(crate) fn image_info(&self, jpeg: &[u8]) -> Result<ImageInfo> {
        let mut components: c_int = 0;
        let mut subsampling = sys::NVJPEG_CSS_UNKNOWN;
        let mut widths = [0 as c_int; sys::NVJPEG_MAX_COMPONENT];
        let mut heights = [0 as c_int; sys::NVJPEG_MAX_COMPONENT];
        // SAFETY: `self.raw` is a live handle. `jpeg` is valid for reads of
        // `jpeg.len()` bytes, and nvJPEG only reads it during the call. The
        // out-pointers are live and writable, and `widths`/`heights` each hold
        // NVJPEG_MAX_COMPONENT ints, which is how many nvJPEG writes.
        check(unsafe {
            sys::nvjpegGetImageInfo(
                self.raw,
                jpeg.as_ptr(),
                jpeg.len(),
                &mut components,
                &mut subsampling,
                widths.as_mut_ptr(),
                heights.as_mut_ptr(),
            )
        })?;

        // Index 0 is the luma plane, the full image size. C ints can be
        // negative, so convert with try_from instead of `as`, which would turn
        // -1 into a huge usize and break buffer-size math later.
        match (
            usize::try_from(widths[0]),
            usize::try_from(heights[0]),
            usize::try_from(components),
        ) {
            (Ok(width), Ok(height), Ok(components)) if width > 0 && height > 0 => Ok(ImageInfo {
                width,
                height,
                components,
                subsampling,
            }),
            _ => Err(Error::NvJpeg(sys::NVJPEG_STATUS_BAD_JPEG)),
        }
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

    /// A header-only baseline JPEG: SOI, an SOF0 frame header, EOI. No image
    /// data, but image_info only parses headers. `sampling` is one
    /// (horizontal << 4 | vertical) byte per component.
    fn jpeg_header(width: u16, height: u16, sampling: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8]; // SOI
        // SOF0 segment length: the length field itself (2), precision (1),
        // height (2), width (2), component count (1), 3 bytes per component.
        let sof_len = 8 + 3 * sampling.len() as u16;
        bytes.extend_from_slice(&[0xFF, 0xC0]); // SOF0
        bytes.extend_from_slice(&sof_len.to_be_bytes()); // JPEG is big-endian
        bytes.push(8); // bits per sample
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.push(sampling.len() as u8);
        for (id, &factors) in (1u8..).zip(sampling) {
            bytes.extend_from_slice(&[id, factors, 0]); // id, sampling, quant table
        }
        bytes.extend_from_slice(&[0xFF, 0xD9]); // EOI
        bytes
    }

    #[test]
    fn image_info_reads_kinect_frame() {
        let handle = Handle::new().expect("nvjpegCreateSimple failed");
        // Kinect color frames: 1920x1080, Y at 2x1, Cb and Cr at 1x1 (4:2:2).
        let header = jpeg_header(1920, 1080, &[0x21, 0x11, 0x11]);
        let info = handle
            .image_info(&header)
            .expect("nvjpegGetImageInfo failed");
        assert_eq!(
            info,
            ImageInfo {
                width: 1920,
                height: 1080,
                components: 3,
                subsampling: sys::NVJPEG_CSS_422,
            }
        );
    }

    #[test]
    fn image_info_rejects_garbage() {
        let handle = Handle::new().expect("nvjpegCreateSimple failed");
        assert!(matches!(
            handle.image_info(&[0xAB; 64]),
            Err(Error::NvJpeg(_))
        ));
        assert!(matches!(handle.image_info(&[]), Err(Error::NvJpeg(_))));
        // A truncated header: SOI marker only.
        assert!(matches!(
            handle.image_info(&[0xFF, 0xD8]),
            Err(Error::NvJpeg(_))
        ));
        // A well-formed header claiming a 0x0 image.
        assert!(matches!(
            handle.image_info(&jpeg_header(0, 0, &[0x21, 0x11, 0x11])),
            Err(Error::NvJpeg(_))
        ));
    }

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
