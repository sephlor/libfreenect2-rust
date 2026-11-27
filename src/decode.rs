use std::fmt;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::time::{Duration, Instant};

#[cfg(feature = "cuda")]
use cuda_core::{Context, DeviceBuffer};
use log::{debug, info, warn};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::error;
#[cfg(feature = "cuda")]
use crate::error::Error;
#[cfg(feature = "cuda")]
use crate::nvjpeg::{self, Handle, State};
use crate::rgb::JpegPacket;

pub struct Frame {
    width: usize,
    height: usize,
    data: Vec<u8>,
    sequence: u32,
    timestamp: u32,
    exposure: f32,
    gain: f32,
    gamma: f32,
}

impl Frame {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    pub fn timestamp(&self) -> u32 {
        self.timestamp
    }

    pub fn exposure(&self) -> f32 {
        self.exposure
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }

    pub fn gamma(&self) -> f32 {
        self.gamma
    }

    pub(crate) fn new(packet: &JpegPacket, width: usize, height: usize, data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            data,
            sequence: packet.sequence(),
            timestamp: packet.timestamp(),
            exposure: packet.exposure(),
            gain: packet.gain(),
            gamma: packet.gamma(),
        }
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("data_len", &self.data.len())
            .field("sequence", &self.sequence)
            .field("timestamp", &self.timestamp)
            .field("exposure", &self.exposure)
            .field("gain", &self.gain)
            .field("gamma", &self.gamma)
            .finish()
    }
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

/// Decodes on the GPU with nvJPEG.
#[cfg(feature = "cuda")]
pub(crate) fn run_gpu(
    packets: Receiver<JpegPacket>,
    frames: SyncSender<Frame>,
) -> error::Result<()> {
    // Retains device 0's primary context, the same one HostPool uses, so the
    // pinned packet buffers are valid here. nvJPEG and the copies share one
    // stream.
    let context = Context::new(0)?;
    let stream = context.new_stream()?;
    let handle = Handle::new()?;
    let mut state = State::new(&handle)?;

    // TODO: replace with a device pool eventually
    let mut output_buffer: Option<DeviceBuffer<u8>> = None;
    let mut dropped = 0u64;

    let (major, minor, patch) = nvjpeg::version()?;
    info!("decoding on the GPU (nvJPEG {major}.{minor}.{patch})");
    for packet in packets {
        let jpeg_bytes = packet.jpeg_data();
        let sequence = packet.sequence();

        let info = match handle.image_info(jpeg_bytes) {
            Ok(info) => info,
            Err(e) => {
                warn!("seq {sequence}: bad jpeg header: {e}");
                continue;
            }
        };
        let size = info.width * info.height * nvjpeg::RGB_BYTES_PER_PIXEL;

        // Reuse the buffer while the size stays the same.
        let output = match output_buffer.take() {
            Some(buffer) if buffer.len() == size => output_buffer.insert(buffer),
            _ => output_buffer.insert(DeviceBuffer::zeroed(&stream, size)?),
        };

        // decode_rgbi synchronizes, so nvJPEG is done reading the packet's
        // pinned buffer before the packet drops (and frees it on another stream).
        match state.decode_rgbi(jpeg_bytes, output, &stream) {
            Ok(_) => {}
            Err(Error::NvJpeg(status)) => {
                warn!("seq {sequence}: decode failed with nvJPEG status {status}");
                continue;
            }
            Err(e) => return Err(e),
        }

        // change this to a new pinned host buffer so we copy to pinned memory, or render from device
        let mut data = vec![0; size];
        output.copy_to_host(&stream, &mut data)?;

        let frame = Frame::new(&packet, info.width, info.height, data);

        match frames.try_send(frame) {
            Err(TrySendError::Disconnected(_)) => {
                log::error!("receiver disconnected, stopping");
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
    Ok(())
}

/// Decodes on the CPU with zune-jpeg.
#[cfg_attr(feature = "cuda", allow(dead_code))]
pub(crate) fn run_cpu(
    packets: Receiver<JpegPacket>,
    frames: SyncSender<Frame>,
) -> error::Result<()> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut dropped = 0u64;

    info!("decoding on the CPU (zune-jpeg)");
    for packet in packets {
        let start = Instant::now();
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(packet.jpeg_data()), options);

        if let Err(e) = decoder.decode_headers() {
            warn!("seq {}: bad jpeg header: {e}", packet.sequence());
            continue;
        }
        let info = decoder.info().expect("headers were decoded");
        let size = decoder.output_buffer_size().expect("headers were decoded");

        let mut pixels = vec![0; size];
        if let Err(e) = decoder.decode_into(&mut pixels) {
            warn!("seq {}: decode failed: {e}", packet.sequence());
            continue;
        }
        debug!(
            "seq {}: decoded in {:.1?}",
            packet.sequence(),
            start.elapsed()
        );

        let frame = Frame::new(
            &packet,
            usize::from(info.width),
            usize::from(info.height),
            pixels,
        );

        match frames.try_send(frame) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                dropped += 1;
                debug!("decode: dropped frame ({dropped} total)");
            }
            Err(TrySendError::Disconnected(_)) => {
                log::error!("receiver disconnected, stopping");
                break;
            }
        }
    }
    debug!("decode thread done, dropped {dropped} frames");
    Ok(())
}
