use std::error::Error;

use libfreenect2_rust::{Kinect, list_kinects};

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .filter_module("nusb", log::LevelFilter::Info)
        .parse_env(env_logger::Env::default())
        .init();

    for k in list_kinects()? {
        println!("found:    {}", k.serial());
    }

    let kinect = Kinect::open_default()?;
    println!("serial:   {}", kinect.serial());
    println!("firmware: {}", kinect.firmware().unwrap_or("unknown"));

    kinect.close()?;
    Ok(())
}
