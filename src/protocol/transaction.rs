//! Send a command and collect its response, on the endpoints of interface 0
//! (layout in the `usb::device` module docs):
//! - 0x02: host -> Kinect
//! - 0x81: Kinect -> host

use std::fmt;

use log::{debug, warn};
use nusb::transfer::{Buffer, Bulk, In, Out};
use nusb::{Endpoint, Interface};

use super::{Command, read_u32};
use crate::error::{self, Error};
use crate::usb::TIMEOUT;

pub(crate) struct CommandTransaction {
    ep_out: Endpoint<Bulk, Out>,
    ep_in: Endpoint<Bulk, In>,
    next_seq: u32,
}

impl fmt::Debug for CommandTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandTransaction")
            .field("next_seq", &self.next_seq)
            .finish_non_exhaustive()
    }
}

impl CommandTransaction {
    const EP_IN: u8 = 0x81;
    const EP_OUT: u8 = 0x02;
    const RESPONSE_COMPLETE_LEN: usize = 16;
    const RESPONSE_COMPLETE_MAGIC: u32 = 0x0A6F_E000;

    pub(crate) fn new(interface: &Interface) -> error::Result<Self> {
        Ok(Self {
            ep_out: interface.endpoint::<Bulk, Out>(Self::EP_OUT)?,
            ep_in: interface.endpoint::<Bulk, In>(Self::EP_IN)?,
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

        if let Some(complete_seq) = read_u32(&complete, 4)
            && complete_seq != seq
        {
            warn!("response complete has sequence {complete_seq}, expected {seq}");
        }

        Ok(response)
    }

    fn send(&mut self, bytes: Vec<u8>) -> error::Result<()> {
        let expected = bytes.len();
        let buffer = Buffer::from(bytes);

        let completion = self.ep_out.transfer_blocking(buffer, TIMEOUT);

        // Bail if completion error
        completion.status?;

        // Second check of length just in case
        let got = completion.actual_len;

        if got != expected {
            return Err(Error::ShortWrite { expected, got });
        }

        Ok(())
    }

    fn receive(&mut self, len: usize) -> error::Result<Vec<u8>> {
        let packet_size = self.ep_in.max_packet_size();
        let requested_len = len.max(1).next_multiple_of(packet_size);
        let buffer = Buffer::new(requested_len);

        let completion = self.ep_in.transfer_blocking(buffer, TIMEOUT);
        let bytes = completion.into_result()?.into_vec();

        Ok(bytes)
    }

    fn is_response_complete(bytes: &[u8]) -> bool {
        bytes.len() == Self::RESPONSE_COMPLETE_LEN
            && read_u32(bytes, 0) == Some(Self::RESPONSE_COMPLETE_MAGIC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_completion_response() {
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
        let bytes: [u8; CommandTransaction::RESPONSE_COMPLETE_LEN] = [
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
