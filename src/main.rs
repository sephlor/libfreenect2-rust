mod error;
mod kinect;
mod protocol;
mod usb;

use std::process::ExitCode;

use error::Error;
use kinect::Kinect;
use log::error;
use usb::KinectUsb;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> error::Result<()> {
    /*
    1. Find Kinects
    2. Open the first Kinect device
    3. Start the device
    4. Print the serial and firmware
    5. Set up interrupt
    6. Loop:
        - receive frames
        - decode
        - display
    7. Stop the device
    8. Close the device
     */

    let kinects = KinectUsb::list_devices()?;
    let kinect_info = kinects.first().ok_or(Error::NoDevice)?;
    let mut kinect = Kinect::open(kinect_info)?;
    kinect.start_rgb()?;

    Ok(())
}
