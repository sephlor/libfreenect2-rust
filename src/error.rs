#[derive(Debug)]
pub enum Error {
    Usb(nusb::Error),
    Transfer(nusb::transfer::TransferError),
    NoDevice,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Usb(e) => write!(f, "Usb error: {e}"),
            Error::Transfer(e) => write!(f, "Usb transfer error: {e}"),
            Error::NoDevice => write!(f, "No device found"),
        }
    }
}

impl std::error::Error for Error {}

impl From<nusb::Error> for Error {
    fn from(e: nusb::Error) -> Self {
        Error::Usb(e)
    }
}

impl From<nusb::transfer::TransferError> for Error {
    fn from(e: nusb::transfer::TransferError) -> Self {
        Error::Transfer(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
