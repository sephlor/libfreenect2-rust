//! Endpoints, see device.rs:
//! 0x02: host -> Kinect
//! 0x81: Kinect -> host

use super::read_u32;
use crate::error::{self, Error};
use crate::protocol::Command;
use log::{debug, warn};
use nusb::transfer::{Buffer, Bulk, In, Out};
use nusb::{Endpoint, Interface};
use std::time::Duration;

pub(crate) struct CommandTransaction {
    out_ep: Endpoint<Bulk, Out>,
    in_ep: Endpoint<Bulk, In>,
    next_seq: u32,
}

impl CommandTransaction {
    const IN_EP: u8 = 0x81;
    const OUT_EP: u8 = 0x02;
    const RESPONSE_COMPLETE_LEN: usize = 16;
    const RESPONSE_COMPLETE_MAGIC: u32 = 0x0A6F_E000;

    const TIMEOUT: Duration = Duration::from_millis(1000);

    pub(crate) fn new(interface: &Interface) -> error::Result<Self> {
        Ok(Self {
            out_ep: interface.endpoint::<Bulk, Out>(Self::OUT_EP)?,
            in_ep: interface.endpoint::<Bulk, In>(Self::IN_EP)?,
            next_seq: 0,
        })
    }

    pub(crate) fn execute(&mut self, cmd: &Command) -> error::Result<Vec<u8>> {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        debug!("executing {cmd:?} seq {seq}");

        // Send the command
        self.send(cmd.to_bytes(seq))?;

        // Receive a response data packet if the command expects one
        let response = if cmd.max_response_len() > 0 {
            let response = self.receive(cmd.max_response_len())?;

            if response.len() < cmd.min_response_len() {
                return Err(Error::ResponseTooShort {
                    expected: cmd.min_response_len(),
                    got: response.len(),
                });
            }
            // Command expected a response packet, not a completion packet
            if Self::is_response_complete(&response) {
                return Err(Error::PrematureResponseComplete);
            }
            response
        } else {
            Vec::new()
        };

        // Receive a completion packet
        let complete = self.receive(Self::RESPONSE_COMPLETE_LEN)?;
        if !Self::is_response_complete(&complete) {
            return Err(Error::MissingResponseComplete);
        }

        let complete_seq = read_u32(&complete, 4);
        if complete_seq != seq {
            warn!("response complete has sequence {complete_seq}, expected {seq}");
        }

        Ok(response)
    }

    fn send(&mut self, bytes: Vec<u8>) -> error::Result<()> {
        let expected = bytes.len();
        let buffer = Buffer::from(bytes);

        let completion = self.out_ep.transfer_blocking(buffer, Self::TIMEOUT);
        completion.status?;

        let got = completion.actual_len;

        if got != expected {
            return Err(Error::ShortWrite { expected, got });
        }

        Ok(())
    }

    fn receive(&mut self, len: usize) -> error::Result<Vec<u8>> {
        let packet_size = self.in_ep.max_packet_size();
        let requested_len = len.max(1).next_multiple_of(packet_size);
        let buffer = Buffer::new(requested_len);

        let completion = self.in_ep.transfer_blocking(buffer, Self::TIMEOUT);
        let bytes = completion.into_result()?.into_vec();

        Ok(bytes)
    }

    fn is_response_complete(bytes: &[u8]) -> bool {
        bytes.len() == Self::RESPONSE_COMPLETE_LEN
            && read_u32(bytes, 0) == Self::RESPONSE_COMPLETE_MAGIC
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_completion_packet() {
        let bytes: [u8; CommandTransaction::RESPONSE_COMPLETE_LEN] = [
            0x00, 0xE0, 0x6F, 0x0A, // RESPONSE_COMPLETE_MAGIC 0x0A6F_E000, little-endian
            0x01, 0x00, 0x00, 0x00, // sequence = 1
            0x00, 0x00, 0x00, 0x00, // unused / unknown
            0x00, 0x00, 0x00, 0x00, // unused / unknown
        ];
        assert!(CommandTransaction::is_response_complete(&bytes));
    }

    #[test]
    fn invalid_completion_response_command() {
        let bytes: [u8; 16] = [
            0x09, 0x20, 0x02, 0x06, // Command::MAGIC 0x0602_2009, little-endian
            0x01, 0x00, 0x00, 0x00, // sequence = 1
            0x00, 0x02, 0x00, 0x00, // max_response_length = 0x200
            0x02, 0x00, 0x00, 0x00, // READ_FIRMWARE_VERSIONS command id = 0x02
        ];
        assert!(!CommandTransaction::is_response_complete(&bytes));
    }

    #[test]
    fn invalid_completion_response_empty() {
        let bytes = [];
        assert!(!CommandTransaction::is_response_complete(&bytes));
    }

    #[test]
    fn invalid_completion_response_too_long() {
        let bytes: [u8; 20] = [
            0x00, 0xE0, 0x6F, 0x0A, // magic 0x0A6F_E000, little-endian
            0x01, 0x00, 0x00, 0x00, // sequence = 1
            0x00, 0x00, 0x00, 0x00, // unused / unknown
            0x00, 0x00, 0x00, 0x00, // unused / unknown
            0x00, 0x00, 0x00, 0x00, // extra
        ];
        assert!(!CommandTransaction::is_response_complete(&bytes));
    }

    #[test]
    fn invalid_completion_response_too_short() {
        let bytes: [u8; 12] = [
            0x00, 0xE0, 0x6F, 0x0A, // magic 0x0A6F_E000, little-endian
            0x01, 0x00, 0x00, 0x00, // sequence = 1
            0x00, 0x00, 0x00, 0x00, // unused / unknown
        ];
        assert!(!CommandTransaction::is_response_complete(&bytes));
    }
}
