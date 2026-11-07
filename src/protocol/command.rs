#[derive(Debug)]
pub(crate) struct Command {
    id: u32,
    max_response_len: u32,
    min_response_len: u32,
    params: Vec<u32>,
}

impl Command {
    const MAGIC: u32 = 0x0602_2009;
    const READ_FIRMWARE_VERSIONS: u32 = 0x02;
    const READ_DATA_PAGE: u32 = 0x22;
    const READ_STATUS: u32 = 0x16;

    pub(super) fn max_response_len(&self) -> usize {
        self.max_response_len as usize
    }

    pub(super) fn min_response_len(&self) -> usize {
        self.min_response_len as usize
    }

    pub(super) fn to_bytes(&self, seq: u32) -> Vec<u8> {
        let header = [Self::MAGIC, seq, self.max_response_len, self.id, 0];
        let mut bytes = Vec::with_capacity((header.len() + self.params.len()) * size_of::<u32>());

        for word in header {
            bytes.extend_from_slice(&word.to_le_bytes());
        }

        for param in &self.params {
            bytes.extend_from_slice(&param.to_le_bytes());
        }

        bytes
    }

    pub(crate) fn read_firmware_versions() -> Self {
        Self {
            id: Self::READ_FIRMWARE_VERSIONS,
            max_response_len: 0x200,
            min_response_len: 0x200,
            params: Vec::new(),
        }
    }

    pub(crate) fn read_serial_number() -> Self {
        Self {
            id: Self::READ_DATA_PAGE,
            max_response_len: 0x80,
            min_response_len: 0x80,
            params: vec![0x01],
        }
    }

    pub(crate) fn read_status(param: u32) -> Self {
        Self {
            id: Self::READ_STATUS,
            max_response_len: 0x04,
            min_response_len: 0x04,
            params: vec![param],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_firmware_versions_bytes() {
        let bytes = Command::read_firmware_versions().to_bytes(1);

        let expected: [u8; 20] = [
            0x09, 0x20, 0x02, 0x06, // magic 0x0602_2009, little-endian
            0x01, 0x00, 0x00, 0x00, // sequence = 1
            0x00, 0x02, 0x00, 0x00, // max_response_length = 0x200
            0x02, 0x00, 0x00, 0x00, // READ_FIRMWARE_VERSIONS command id = 0x02
            0x00, 0x00, 0x00, 0x00, // reserved
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn read_serial_number_bytes() {
        let bytes = Command::read_serial_number().to_bytes(5);

        let expected: [u8; 24] = [
            0x09, 0x20, 0x02, 0x06, // magic 0x0602_2009, little-endian
            0x05, 0x00, 0x00, 0x00, // sequence = 5
            0x80, 0x00, 0x00, 0x00, // max_response_length = 0x80
            0x22, 0x00, 0x00, 0x00, // READ_DATA_PAGE command id = 0x22
            0x00, 0x00, 0x00, 0x00, // reserved
            0x01, 0x00, 0x00, 0x00, // param = 0x01
        ];
        assert_eq!(bytes, expected);
    }
}
