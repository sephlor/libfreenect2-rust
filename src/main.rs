use cuda_async::device_context::{init_device_contexts, with_device};
use cuda_core::sys;
use log::error;
use std::{process::ExitCode, thread::sleep, time::Duration};

mod error;
mod kinect;
mod pinned;
mod protocol;
mod rgb;
mod usb;
use kinect::Kinect;

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
    init_device_contexts(0, 1).expect("init failed (requires GPU)");

    // SAFETY: CUmemPoolProps is a plain C struct; all-zero is a valid starting value.
    let mut props: sys::CUmemPoolProps = unsafe { std::mem::zeroed() };
    props.allocType = sys::CUmemAllocationType_enum_CU_MEM_ALLOCATION_TYPE_PINNED;
    props.handleTypes = sys::CUmemAllocationHandleType_enum_CU_MEM_HANDLE_TYPE_NONE;
    props.location.type_ = sys::CUmemLocationType_enum_CU_MEM_LOCATION_TYPE_HOST;

    let pool = with_device(0, |device| device.new_mem_pool_with_params(props))??;
    pool.set_release_threshold(u64::MAX)?;

    let mut kinect = Kinect::open_default_device()?;
    kinect.start_rgb(pool)?;

    sleep(Duration::from_secs(5));

    kinect.stop()?;

    kinect.close()?;

    Ok(())
}
