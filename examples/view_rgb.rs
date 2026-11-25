mod common;

use std::error::Error;
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;

use log::error;

use common::render;
use libfreenect2_rust::{Frame, HostPool, JpegPacket, Kinect, RgbStreamParser, run_cpu};

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
    let pool = HostPool::new()?;

    let parser = RgbStreamParser::new(pool);

    let (packet_tx, packet_rx) = mpsc::sync_channel::<JpegPacket>(1);

    let mut kinect = Kinect::open_default()?;

    // TODO: pass a closure instead?
    kinect.start_rgb(parser, packet_tx)?;

    let (frame_tx, frame_rx) = mpsc::sync_channel::<Frame>(1);

    let decode_thread = thread::Builder::new()
        .name("decode".into())
        .spawn(move || run_cpu(packet_rx, frame_tx))?;

    let render_result = render::run(frame_rx);

    kinect.stop()?;

    if decode_thread.join().is_err() {
        error!("decode thread panicked");
    }

    kinect.close()?;

    render_result
}
