use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::time::{Duration, Instant};

#[cfg(feature = "cuda")]
use baracuda_driver::{Context, Device, DeviceBuffer, PrimaryContext};
#[cfg(feature = "cuda")]
use baracuda_nvjpeg::{Handle, PixelFormat, State, decode_interleaved};
use log::{debug, error, info, warn};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::rgb::JpegPacket;

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
    pub sequence: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RecvError {
    #[error("no frame ready")]
    Empty,

    #[error("stream stopped")]
    Disconnected,
}

#[derive(Debug)]
pub struct FrameReceiver {
    frames: Receiver<Frame>,
}

impl FrameReceiver {
    pub(crate) fn new(frames: Receiver<Frame>) -> Self {
        Self { frames }
    }

    pub fn recv(&self) -> Result<Frame, RecvError> {
        self.frames.recv().map_err(|_| RecvError::Disconnected)
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Frame, RecvError> {
        self.frames.recv_timeout(timeout).map_err(|e| match e {
            RecvTimeoutError::Timeout => RecvError::Empty,
            RecvTimeoutError::Disconnected => RecvError::Disconnected,
        })
    }

    pub fn try_recv(&self) -> Result<Frame, RecvError> {
        self.frames.try_recv().map_err(|e| match e {
            TryRecvError::Empty => RecvError::Empty,
            TryRecvError::Disconnected => RecvError::Disconnected,
        })
    }
}

#[cfg(feature = "cuda")]
pub(crate) fn run_gpu(packets: Receiver<JpegPacket>, frames: SyncSender<Frame>) {
    // TODO: do something besides unwrap
    let device = Device::get(0).unwrap();
    let primary = PrimaryContext::retain(&device).unwrap();
    // SAFETY: `primary` keeps the device's primary context alive for the whole
    // function. `context` and every clone of it (the one inside `output_buffer`)
    // are declared after `primary`, so they drop before it does.
    let context = unsafe { Context::borrow_raw(primary.as_raw(), &device) };
    // TODO: this uses hybrid backend only, try swapping out backends with raw FFI or nvjpeg-sys
    let handle = Handle::new().unwrap();
    let state = State::new(&handle).unwrap();

    // TODO: replace with a device pool eventually
    let mut output_buffer: Option<DeviceBuffer<u8>> = None;
    let mut dropped = 0u64;

    info!("decoding on the GPU (nvJPEG)");
    for packet in packets {
        let jpeg_bytes = packet.jpeg_data();
        let info = handle.image_info(jpeg_bytes).unwrap();
        let width = info.width();
        let height = info.height();
        let format = PixelFormat::Rgbi;
        let size = width as usize * height as usize * format.bytes_per_pixel();
        let sequence = *packet.sequence();

        if output_buffer.as_ref().map(DeviceBuffer::len) != Some(size) {
            output_buffer = Some(DeviceBuffer::new(&context, size).unwrap());
        }
        let output = output_buffer.as_mut().unwrap();

        if let Err(e) = decode_interleaved(&handle, &state, jpeg_bytes, format, output, width, None)
        {
            warn!("seq {sequence}: decode failed: {e}");
            continue;
        }

        // change this to a new pinned host buffer so we copy to pinned memory, or render from device
        let mut data = vec![0; size];

        output.copy_to_host(&mut data).unwrap();

        let frame = Frame {
            sequence,
            width: width as usize,
            height: height as usize,
            data,
        };

        match frames.try_send(frame) {
            Err(TrySendError::Disconnected(_)) => {
                error!("receiver disconnected, stopping");
                break;
            }
            Err(TrySendError::Full(_)) => {
                dropped += 1;
                debug!("decode: dropped frame ({dropped} total)");
            }
            Ok(()) => {}
        }
    }
    debug!("decode thread done, dropped {dropped} frames");
}

// Option to decode on CPU for performance comparison
#[allow(dead_code)]
pub(crate) fn run_cpu(packets: Receiver<JpegPacket>, frames: SyncSender<Frame>) {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut dropped = 0u64;

    info!("decoding on the CPU (zune-jpeg)");
    for pkt in packets {
        let start = Instant::now();
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(pkt.jpeg_data()), options);

        if let Err(e) = decoder.decode_headers() {
            warn!("seq {}: bad jpeg header: {e}", pkt.sequence());
            continue;
        }
        let info = decoder.info().expect("headers were decoded");
        let size = decoder.output_buffer_size().expect("headers were decoded");

        let mut pixels = vec![0; size];
        if let Err(e) = decoder.decode_into(&mut pixels) {
            warn!("seq {}: decode failed: {e}", pkt.sequence());
            continue;
        }
        debug!("seq {}: decoded in {:.1?}", pkt.sequence(), start.elapsed());

        let frame = Frame {
            sequence: *pkt.sequence(),
            width: usize::from(info.width),
            height: usize::from(info.height),
            data: pixels,
        };

        match frames.try_send(frame) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                dropped += 1;
                debug!("decode: dropped frame ({dropped} total)");
            }
            Err(TrySendError::Disconnected(_)) => {
                error!("receiver disconnected, stopping");
                break;
            }
        }
    }
    debug!("decode thread done, dropped {dropped} frames");
}
