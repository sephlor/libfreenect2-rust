mod command;
pub(crate) mod response;
mod transaction;

pub(crate) use command::Command;
pub(crate) use transaction::CommandTransaction;

/// Reads a little-endian u32 at `offset`.
/// Panics if `bytes` is shorter than `offset + 4`; callers check the length first.
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
