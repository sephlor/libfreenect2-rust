//! USB access to the Kinect: discovery, open/reset, and standard control requests.

pub(crate) mod bulk_stream;
mod control;
mod device;

use std::time::Duration;

pub(crate) use device::KinectUsb;
pub(crate) const TIMEOUT: Duration = Duration::from_secs(1);
