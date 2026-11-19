use std::thread;
use std::{process::ExitCode, sync::mpsc};

use log::error;

use crate::decode::Frame;
use crate::kinect::Kinect;
use crate::pool::HostPool;
use crate::rgb::{JpegPacket, RgbStreamParser};

mod buffer;
mod decode;
mod error;
mod kinect;
mod pool;
mod protocol;
mod rgb;
mod usb;

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

    let (frame_tx, frame_rx) = mpsc::sync_channel::<Frame>(1);

    let decode_thread = thread::Builder::new()
        .name("decode".into())
        .spawn(move || decode::run(packet_rx, frame_tx))?;

    // pass frame_rx and render

    kinect.stop()?;

    if decode_thread.join().is_err() {
        error!("decode thread panicked");
    }

    kinect.close()?;

    Ok(())
}
