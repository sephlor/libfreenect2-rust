use log::error;
use std::{process::ExitCode, sync::mpsc};

mod buffer;
mod error;
mod kinect;
mod pool;
mod protocol;
mod rgb;
mod usb;
use kinect::Kinect;
use rgb::JpegPacket;

use crate::{pool::HostPool, rgb::RgbStreamParser};

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

fn run() -> error::Result<()> {
    let pool = HostPool::new()?;

    let parser = RgbStreamParser::new(pool);

    let (packet_tx, packet_rx) = mpsc::sync_channel::<JpegPacket>(1);

    let mut kinect = Kinect::open_default_device()?;

    // TODO: pass a closure instead?
    kinect.start_rgb(parser, packet_tx)?;

    for pkt in packet_rx.iter().take(5) {
        std::fs::write(
            format!("output/frame_{:05}.jpg", pkt.sequence()),
            pkt.jpeg_data(),
        )?;
    }

    kinect.stop()?;

    kinect.close()?;

    Ok(())
}
