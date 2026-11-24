use std::sync::mpsc::{Receiver, SyncSender, TrySendError};

use baracuda_driver::{Context, Device, DeviceBuffer, PrimaryContext};
use baracuda_nvjpeg::{Handle, PixelFormat, State, decode_interleaved};
use log::{debug, error, warn};

use crate::rgb::JpegPacket;

pub(crate) struct Frame {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) data: Vec<u8>,
    pub(crate) sequence: u32,
}

pub(crate) fn run(packets: Receiver<JpegPacket>, frames: SyncSender<Frame>) {
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
            Err(TrySendError::Full(_)) => debug!("receiver full, dropping frame"),
            Ok(()) => {}
        }
    }
}
