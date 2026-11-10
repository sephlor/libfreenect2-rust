//! Reassembles complete MJPEG frames from the color camera's bulk stream.
//!
//! Output is compressed JPEG bytes, not decoded pixels.
//!
//! This is the same thing as RgbPacketStreamParser in libfreenect2.

use log::{trace, warn};

use crate::protocol::read_u32_from_offset;

const MAGIC_JPEG: u32 = 0x42424242; // decimal: 66,66,66,66 // ASCII: BBBB
const MAGIC_FOOTER_START: u32 = 0x39393939; // decimal: 57,57,57,57 // ASCII: 9999

const RGB_HEADER_SIZE: usize = size_of::<RawRgbPacketHeader>(); // 8
const RGB_FOOTER_SIZE: usize = size_of::<RawRgbPacketFooter>(); // 56

/// Accumulates raw bulk transfer chunks and extracts one complete JPEG frame
/// per packet, along with its sequence, timestamp, and exposure metadata.
pub(crate) struct RgbStreamParser {
    packet: Option<JpegPacket>,
}

#[repr(C)]
struct RawRgbPacketHeader {
    sequence: u32,
    magic_jpeg_header: u32, // MAGIC_JPEG
}

#[repr(C)]
struct RawRgbPacketFooter {
    magic_footer_start: u32, // MAGIC_FOOTER_START
    sequence: u32,
    filler_length: u32,
    unknown1: u32, // seems 0 always
    unknown2: u32, // seems 0 always
    timestamp: u32,
    exposure: f32, // ? ranges from 0.5 to about 60.0 with powerfull light at camera or totally covered
    gain: f32,     // ? ranges from 1.0 when camera is clear to 1.5 when camera is covered.
    magic_jpeg: u32, // MAGIC_JPEG
    packet_size: u32,
    gamma: f32,         // ranges from 1.0f to about 6.4 when camera is fully covered
    unknown4: [u32; 3], // seems to be 0 all the time
}

#[derive(Default)]
struct JpegPacket {
    sequence: u32,
    timestamp: u32,
    jpeg_data: Vec<u8>,
    jpeg_data_length: usize,
    exposure: f32,
    gain: f32,
    gamma: f32,
}

impl RgbStreamParser {
    const BUFFER_SIZE: usize = 0x200000; // 2 MB // 2*1024*1024

    pub(crate) fn new() -> Self {
        Self { packet: None }
    }

    pub(crate) fn parse_packets(&mut self, data: &[u8]) {
        if self.packet.is_none() {
            // first two words in RawRgbPacketHeader, so 0 and 4 offsets
            let (Some(sequence), Some(magic_jpeg)) =
                (read_u32_from_offset(data, 0), read_u32_from_offset(data, 4))
            else {
                return;
            };
            if magic_jpeg == MAGIC_JPEG {
                // TODO: swap for cudaMallocFromPoolAsync
                let mut jpeg_data = Vec::with_capacity(Self::BUFFER_SIZE);
                jpeg_data.extend_from_slice(&data[RGB_HEADER_SIZE..]);
                self.packet = Some(JpegPacket {
                    sequence,
                    jpeg_data,
                    ..Default::default()
                });
            }
        } else if let Some(mut packet) = self.packet.take() {
            if let Some(footer) = RawRgbPacketFooter::from_bytes(data) {
                let jpeg_packet = Self::get_jpeg_packet(packet, data, footer);

                // TODO: send packet
            } else {
                if packet.jpeg_data.len() + data.len() > Self::BUFFER_SIZE {
                    warn!("buffer overflow!");
                } else {
                    packet.jpeg_data.extend_from_slice(data);
                    self.packet.replace(packet);
                }
            }
        }
    }

    fn get_jpeg_packet(
        mut packet: JpegPacket,
        data: &[u8],
        footer: RawRgbPacketFooter,
    ) -> Option<JpegPacket> {
        if footer.sequence == packet.sequence {
            let packet_size = RGB_HEADER_SIZE + packet.jpeg_data.len() + data.len();
            if packet_size == footer.packet_size as usize {
                // if filler overuns into previous chunk remove filler from jpeg_data
                if (footer.filler_length as usize + RGB_FOOTER_SIZE) >= data.len() {
                    let prev_filler_bytes =
                        footer.filler_length as usize + RGB_FOOTER_SIZE - data.len();

                    packet
                        .jpeg_data
                        .truncate(packet.jpeg_data.len().saturating_sub(prev_filler_bytes));
                } else {
                    // copy jpeg tail to jpeg_data
                    let filler_start = data.len() - RGB_FOOTER_SIZE - footer.filler_length as usize;
                    let jpeg_tail = &data[..filler_start];
                    if packet.jpeg_data.len() + jpeg_tail.len() > Self::BUFFER_SIZE {
                        warn!("buffer overflow!");
                        return None;
                    } else {
                        packet.jpeg_data.extend_from_slice(jpeg_tail);
                    }
                }

                let mut eoi = 0;
                for i in 0..4 {
                    let idx = packet.jpeg_data.len().saturating_sub(i);
                    if idx >= 2
                        && packet.jpeg_data[idx - 2] == 0xff
                        && packet.jpeg_data[idx - 1] == 0xd9
                    {
                        eoi = idx;
                    }
                }
                if eoi > 0 {
                    // remove padding & set RgbPacket fields
                    packet.jpeg_data.truncate(eoi);
                    packet.jpeg_data_length = packet.jpeg_data.len();
                    packet.timestamp = footer.timestamp;
                    packet.exposure = footer.exposure;
                    packet.gain = footer.gain;
                    packet.gamma = footer.gamma;
                    trace!(
                        "found a packet! seq {}, jpeg {} bytes",
                        packet.sequence, packet.jpeg_data_length
                    );
                    return Some(packet);
                }
            }
        }
        warn!("dropped a packet");
        None
    }
}

impl RawRgbPacketFooter {
    fn from_bytes(data: &[u8]) -> Option<RawRgbPacketFooter> {
        if let Some(last_chunk) = data.last_chunk::<RGB_FOOTER_SIZE>() {
            let footer = Self::from_chunk(last_chunk);
            // check if footer
            if footer.magic_footer_start == MAGIC_FOOTER_START && footer.magic_jpeg == MAGIC_JPEG {
                return Some(footer);
            }
        }
        None
    }

    fn from_chunk(bytes: &[u8; 56]) -> RawRgbPacketFooter {
        let (words, _rest) = bytes.as_chunks::<4>();

        let word_to_u32 = |b: usize| u32::from_le_bytes(words[b]);
        let word_to_f32 = |b: usize| f32::from_le_bytes(words[b]);

        RawRgbPacketFooter {
            magic_footer_start: word_to_u32(0),
            sequence: word_to_u32(1),
            filler_length: word_to_u32(2),
            unknown1: word_to_u32(3),
            unknown2: word_to_u32(4),
            timestamp: word_to_u32(5),
            exposure: word_to_f32(6),
            gain: word_to_f32(7),
            magic_jpeg: word_to_u32(8),
            packet_size: word_to_u32(9),
            gamma: word_to_f32(10),
            unknown4: [word_to_u32(11), word_to_u32(12), word_to_u32(13)],
        }
    }
}
