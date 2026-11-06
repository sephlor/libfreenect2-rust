mod error;
mod usb;

use std::process::ExitCode;

use error::Error;
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
    let kinects = KinectUsb::list_devices()?;
    let kinect_info = kinects.first().ok_or(Error::NoDevice)?;

    let kinect = KinectUsb::open(kinect_info)?;

    Ok(())
}
