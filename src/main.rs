mod error;
mod kinect;
mod protocol;
mod usb;

use kinect::Kinect;
use log::error;
use std::{process::ExitCode, thread::sleep, time::Duration};

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
    let mut kinect = Kinect::open_default_device()?;
    kinect.start_rgb()?;

    sleep(Duration::from_secs(7));

    kinect.stop_streaming()?;
    kinect.close()?;

    Ok(())
}
