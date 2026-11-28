#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("IO error: {0}")]
    ThreadSpawn(#[from] std::io::Error),

    #[error("USB error: {0}")]
    Usb(#[source] UsbError),

    #[cfg(feature = "cuda")]
    #[error("CUDA driver error: {0}")]
    CudaDriver(#[from] cuda_core::DriverError),

    #[cfg(feature = "cuda")]
    #[error("CUDA device error: {0}")]
    CudaDevice(#[from] cuda_async::error::DeviceError),

    #[cfg(feature = "cuda")]
    #[error("nvJPEG call failed with status {0}")]
    NvJpeg(std::ffi::c_int),

    #[cfg(feature = "cuda")]
    #[error("decode output buffer too small: need {needed} bytes, got {got}")]
    OutputTooSmall { needed: usize, got: usize },

    #[error("no Kinect device found")]
    NoDevice,

    #[error("no Kinect found with serial number {serial}")]
    DeviceNotFound { serial: String },

    #[error("Kinect has no USB serial number")]
    NoSerialNumber,

    #[error("response too short: expected at least {expected} bytes, got {got}")]
    ResponseTooShort { expected: usize, got: usize },

    #[error("short write: expected to send {expected} bytes, sent {got}")]
    ShortWrite { expected: usize, got: usize },

    #[error("received response complete before the response data")]
    PrematureResponseComplete,

    #[error("missing or malformed response complete")]
    MissingResponseComplete,

    #[error("device closed")]
    DeviceClosed,

    #[error("already streaming, if this is a mistake an earlier stop call may have failed")]
    AlreadyStreaming,
}

pub type Result<T> = std::result::Result<T, Error>;

/// USB failures not part of public API.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct UsbError(UsbErrorKind);

#[derive(Debug, thiserror::Error)]
enum UsbErrorKind {
    #[error(transparent)]
    Usb(#[from] nusb::Error),

    #[error("transfer failed: {0}")]
    Transfer(#[from] nusb::transfer::TransferError),
}

#[doc(hidden)]
impl From<nusb::Error> for Error {
    fn from(e: nusb::Error) -> Self {
        Error::Usb(UsbError(e.into()))
    }
}

#[doc(hidden)]
impl From<nusb::transfer::TransferError> for Error {
    fn from(e: nusb::transfer::TransferError) -> Self {
        Error::Usb(UsbError(e.into()))
    }
}
