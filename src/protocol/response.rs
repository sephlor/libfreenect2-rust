use super::read_u32;

pub(crate) fn parse_serial(data: &[u8]) -> String {
    data.iter()
        .step_by(2)
        .take_while(|&&b| b > 0)
        .map(|&b| char::from(b))
        .collect()
}

pub(crate) fn parse_firmware(data: &[u8]) -> Option<String> {
    let record = data.as_chunks::<16>().0.get(3)?;

    let maj_min = read_u32(record, 0)?;
    let revision = read_u32(record, 4)?;
    let build = read_u32(record, 8)?;
    let maj = maj_min >> 16;
    let min = maj_min & 0xffff;

    Some(format!("{maj}.{min}.{revision}.{build}"))
}

pub(crate) fn parse_status(data: &[u8]) -> Option<u32> {
    read_u32(data, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_serial_bytes() {
        let expected = "166506435047";

        let bytes = [
            49, 0, 54, 0, 54, 0, 53, 0, 48, 0, 54, 0, 52, 0, 51, 0, 53, 0, 48, 0, 52, 0, 55, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];

        assert_eq!(parse_serial(&bytes), expected);
    }

    #[test]
    fn parse_serial_bytes_empty() {
        assert_eq!(parse_serial(&[]), "");
    }

    #[test]
    fn parse_firmware_bytes() {
        let expected = "4.0.3917.0";

        let bytes: [u8; 80] = [
            1, 0, 1, 0, 90, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 0: 1.1.2906.0
            0, 0, 4, 0, 77, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 1: 4.0.3917.0
            0, 0, 4, 0, 77, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 2: 4.0.3917.0
            0, 0, 4, 0, 77, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 3: 4.0.3917.0
            1, 0, 1, 0, 90, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 4: 1.1.2906.0
        ];

        assert_eq!(parse_firmware(&bytes), Some(expected.to_string()));
    }

    #[test]
    fn parse_firmware_bytes_empty() {
        assert_eq!(parse_firmware(&[]), None);
    }

    #[test]
    fn parse_firmware_fewer_than_4_records() {
        let bytes: [u8; 48] = [
            1, 0, 1, 0, 90, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 0: 1.1.2906.0
            0, 0, 4, 0, 77, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 1: 4.0.3917.0
            0, 0, 4, 0, 77, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 2: 4.0.3917.0
        ];

        assert_eq!(parse_firmware(&bytes), None);
    }

    #[test]
    fn parse_firmware_uses_record_3() {
        let expected = "1.1.2906.0";

        let bytes: [u8; 64] = [
            1, 0, 1, 0, 90, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 0: 1.1.2906.0
            0, 0, 4, 0, 77, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 1: 4.0.3917.0
            0, 0, 4, 0, 77, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 2: 4.0.3917.0
            1, 0, 1, 0, 90, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // record 3: 1.1.2906.0
        ];

        assert_eq!(parse_firmware(&bytes), Some(expected.to_string()));
    }

    #[test]
    fn parse_status_bytes() {
        let expected = 0x2601; // bit 0 set: ready

        let bytes: [u8; 4] = [0x01, 0x26, 0x00, 0x00];

        assert_eq!(parse_status(&bytes), Some(expected));
    }

    #[test]
    fn parse_status_bytes_empty() {
        assert_eq!(parse_status(&[]), None);
    }

    #[test]
    fn parse_status_fewer_than_4_bytes() {
        let bytes: [u8; 3] = [0x01, 0x26, 0x00];

        assert_eq!(parse_status(&bytes), None);
    }

    #[test]
    fn parse_status_ignores_extra_bytes() {
        let expected = 0x2601; // bit 0 set: ready

        let bytes: [u8; 5] = [0x01, 0x26, 0x00, 0x00, 0x14];

        assert_eq!(parse_status(&bytes), Some(expected));
    }
}
