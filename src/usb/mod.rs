//! USB access to the Kinect: discovery, open/reset, and standard control requests.

mod control;
mod device;

use std::time::Duration;

pub(crate) use device::KinectUsb;

pub(crate) const TIMEOUT: Duration = Duration::from_millis(1000);
