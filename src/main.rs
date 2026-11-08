mod error;
mod kinect;
mod protocol;
mod usb;

use std::process::ExitCode;

use log::error;

use kinect::Kinect;

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

    let mut kinect = Kinect::open_default_device()?;
    kinect.start_rgb()?;

    Ok(())
}
