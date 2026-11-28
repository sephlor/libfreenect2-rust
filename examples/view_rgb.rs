mod common;

use std::error::Error;
use std::process::ExitCode;

use libfreenect2_rust::Kinect;
use log::error;

use crate::common::render;

fn main() -> ExitCode {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .filter_module("nusb", log::LevelFilter::Info)
        .parse_env(env_logger::Env::default())
        .init();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut kinect = Kinect::open_default()?;

    let frames = kinect.start()?;

    let render_result = render::run(frames);

    kinect.stop()?;
    kinect.close()?;

    render_result
}
