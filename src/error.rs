#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("USB error: {0}")]
    Usb(#[from] nusb::Error),

    #[error("USB transfer error: {0}")]
    Transfer(#[from] nusb::transfer::TransferError),

    #[error("no Kinect device found")]
    NoDevice,

    #[error("response too short: expected at least {expected} bytes, got {got}")]
    ResponseTooShort { expected: usize, got: usize },

    #[error("short write: expected to send {expected} bytes, sent {got}")]
    ShortWrite { expected: usize, got: usize },

    #[error("received response complete before the response data")]
    PrematureResponseComplete,

    #[error("missing or malformed response complete")]
    MissingResponseComplete,
}

pub type Result<T> = std::result::Result<T, Error>;
