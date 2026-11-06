mod error;
mod usb;

use error::Error;
use usb::KinectUsb;

// TODO: consider printing errors with Display instead of Debug.
// Move this body into `fn run() -> error::Result<()>`, then have
// main match on it, `eprintln!("Error: {e}")`, and return ExitCode::FAILURE.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kinects = KinectUsb::list_devices()?;

    let kinect_info = kinects.first().ok_or(Error::NoDevice)?;

    println!("First KinectUsb: {:?}", kinect_info);

    let kinect = KinectUsb::open(kinect_info)?;

    println!("First KinectUsb post setup: {:?}", kinect);

    Ok(())
}
