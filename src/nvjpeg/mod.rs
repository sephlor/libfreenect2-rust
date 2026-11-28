//! Safe wrapper over the hand-written nvJPEG bindings in `sys`.
use std::ffi::c_int;
use std::ptr;

use cuda_core::{DeviceBuffer, Stream};
use log::warn;

use crate::error::{Error, Result};

mod sys;

/// Bytes per pixel of `NVJPEG_OUTPUT_RGBI`.
pub(crate) const RGB_BYTES_PER_PIXEL: usize = 3;

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
pub(crate) struct Handle {
    raw: sys::nvjpegHandle_t,
}

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
/// Holding `&'h Handle` does two things: the compiler rejects dropping the
/// handle while a state still exists (the C API requires that order), and
/// `decode` always uses the handle this state was made from, so the two can't
/// be mismatched.
pub(crate) struct State<'h> {
    raw: sys::nvjpegJpegState_t,
    handle: &'h Handle,
}

impl<'h> State<'h> {
    pub(crate) fn new(handle: &'h Handle) -> Result<Self> {
        let mut raw = ptr::null_mut();
        // SAFETY: `handle.raw` is a live handle (borrowed for 'h), and `raw` is
        // a writable out-pointer that receives the new state on success.
        check(unsafe { sys::nvjpegJpegStateCreate(handle.raw, &mut raw) })?;
        Ok(Self { raw, handle })
    }

    /// Decodes `jpeg` to interleaved RGB (3 bytes per pixel, rows packed with
    /// no padding) at the start of `output`, on `stream`. Returns the header
    /// info so the caller knows the image size.
    ///
    /// Blocks until the decode finishes. nvJPEG may still read `jpeg` and
    /// write `output` from GPU work after `nvjpegDecode` returns, and the
    /// borrows end when this function does, so it must not return early.
    /// Overlapping decode with other work (events instead of a sync) is a
    /// later step.
    ///
    /// `&mut self`: a state holds one decode's intermediate buffers, so two
    /// decodes can't share it at once.
    pub(crate) fn decode_rgbi(
        &mut self,
        jpeg: &[u8],
        output: &mut DeviceBuffer<u8>,
        stream: &Stream,
    ) -> Result<ImageInfo> {
        let info = self.handle.image_info(jpeg)?;
        // C can't see Rust lengths, so this check is what keeps nvJPEG from
        // writing past the end of `output`. checked_mul guards the math
        // against a header that claims an absurd size.
        let pitch = info.width.checked_mul(RGB_BYTES_PER_PIXEL);
        let needed = pitch.and_then(|p| p.checked_mul(info.height));
        let (Some(pitch), Some(needed)) = (pitch, needed) else {
            return Err(Error::NvJpeg(sys::NVJPEG_STATUS_BAD_JPEG));
        };
        if output.len() < needed {
            return Err(Error::OutputTooSmall {
                needed,
                got: output.len(),
            });
        }

        let mut destination = sys::nvjpegImage_t {
            channel: [ptr::null_mut(); sys::NVJPEG_MAX_COMPONENT],
            pitch: [0; sys::NVJPEG_MAX_COMPONENT],
        };
        destination.channel[0] = output.cu_deviceptr() as *mut u8;
        destination.pitch[0] = pitch;

        // nvJPEG writes `output` behind cuda-core's back. A write lease tells
        // cuda-core's access tracking about it, so it rejects a conflicting
        // access to this buffer from another stream while the decode runs.
        let _lease = output.storage().acquire(stream, true, false)?;
        // nvJPEG goes through the CUDA runtime, which uses the context current
        // on this thread. Make it the stream's context (the primary context).
        stream.context().bind_to_thread()?;

        // SAFETY: `self.raw` and `self.handle.raw` are live and belong
        // together (see the struct docs). `jpeg` is valid for `jpeg.len()`
        // bytes. `channel[0]` points to a device allocation of at least
        // `needed` = pitch * height bytes (checked above) in the stream's
        // context, and the other channels are unused for RGBI. `stream` is a
        // live stream in the same context. The synchronize below keeps `jpeg`,
        // `output` and the lease alive until the GPU work is done.
        let status = unsafe {
            sys::nvjpegDecode(
                self.handle.raw,
                self.raw,
                jpeg.as_ptr(),
                jpeg.len(),
                sys::NVJPEG_OUTPUT_RGBI,
                &mut destination,
                stream.cu_stream(),
            )
        };
        // Wait even if the call failed: it may have queued work before failing.
        let synced = stream.synchronize();
        check(status)?;
        synced?;
        Ok(info)
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

    fn stream() -> std::sync::Arc<Stream> {
        let context = cuda_core::Context::new(0).expect("no CUDA device 0");
        context.new_stream().expect("failed to create stream")
    }

    #[test]
    fn decode_rejects_small_output() {
        let handle = Handle::new().expect("nvjpegCreateSimple failed");
        let mut state = State::new(&handle).expect("nvjpegJpegStateCreate failed");
        let stream = stream();
        // The size check runs before nvjpegDecode, so a header is enough.
        let header = jpeg_header(1920, 1080, &[0x21, 0x11, 0x11]);
        let mut output = DeviceBuffer::<u8>::zeroed(&stream, 1920 * 1080 * 3 - 1).unwrap();
        assert!(matches!(
            state.decode_rgbi(&header, &mut output, &stream),
            Err(Error::OutputTooSmall {
                needed: 6_220_800,
                got: 6_220_799
            })
        ));
    }

    /// Mean absolute difference per byte between two equal-length images.
    fn mean_diff(a: &[u8], b: &[u8]) -> f64 {
        assert_eq!(a.len(), b.len());
        let total: u64 = a
            .iter()
            .zip(b)
            .map(|(&x, &y)| u64::from(x.abs_diff(y)))
            .sum();
        total as f64 / a.len() as f64
    }

    /// Encodes a synthetic image in memory with 4:2:2 sampling like the
    /// Kinect's, decodes it on the GPU, and compares against the source pixels
    /// and against zune-jpeg. jpeg-encoder is a dev-dependency only.
    #[test]
    fn decode_matches_source_and_cpu_decoder() {
        use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
        use zune_jpeg::JpegDecoder;
        use zune_jpeg::zune_core::bytestream::ZCursor;
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;

        // Not square, so swapped width/height shows up. Each channel follows
        // a different pattern (R across, G down, B constant), so swapped
        // channels show up. Smooth gradients keep JPEG's loss small.
        let (width, height) = (640, 360);
        let source: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width)
                    .flat_map(move |x| [(x * 255 / width) as u8, (y * 255 / height) as u8, 64])
            })
            .collect();
        let mut jpeg = Vec::new();
        let mut encoder = Encoder::new(&mut jpeg, 95);
        encoder.set_sampling_factor(SamplingFactor::F_2_1); // 4:2:2
        encoder
            .encode(&source, width as u16, height as u16, ColorType::Rgb)
            .unwrap();

        let handle = Handle::new().expect("nvjpegCreateSimple failed");
        let mut state = State::new(&handle).expect("nvjpegJpegStateCreate failed");
        let stream = stream();
        let mut output = DeviceBuffer::<u8>::zeroed(&stream, source.len()).unwrap();
        let info = state.decode_rgbi(&jpeg, &mut output, &stream).unwrap();
        assert_eq!((info.width, info.height), (640, 360));
        assert_eq!(info.subsampling, sys::NVJPEG_CSS_422);
        let gpu = output.to_host_vec(&stream).unwrap();

        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
        let cpu = JpegDecoder::new_with_options(ZCursor::new(&jpeg), options)
            .decode()
            .unwrap();

        // JPEG is lossy, and the two decoders use different IDCT and chroma
        // upsampling, so neither match is exact. Swapped channels, a wrong
        // pitch or an undecoded (zeroed) buffer put the mean in the tens.
        let vs_source = mean_diff(&gpu, &source);
        let vs_cpu = mean_diff(&gpu, &cpu);
        println!("mean diff: vs source {vs_source:.3}, vs zune-jpeg {vs_cpu:.3}");
        assert!(vs_source < 2.0, "vs source: {vs_source:.3}");
        assert!(vs_cpu < 1.0, "vs zune-jpeg: {vs_cpu:.3}");
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
