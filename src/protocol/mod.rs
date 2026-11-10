//! The Kinect command protocol on bulk endpoints 0x02 / 0x81.

mod command;
pub(crate) mod response;
mod transaction;

pub(crate) use command::Command;
pub(crate) use transaction::CommandTransaction;

/// Reads a little-endian u32 at `offset`, or `None` if `bytes` is too short.
pub(crate) fn read_u32_from_offset(bytes: &[u8], offset: usize) -> Option<u32> {
    let word = bytes.get(offset..)?.first_chunk::<4>()?;
    Some(u32::from_le_bytes(*word))
}
