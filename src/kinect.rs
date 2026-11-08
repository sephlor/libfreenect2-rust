use crate::error::{self, Error};
use crate::protocol::response;
use crate::protocol::{Command, CommandTransaction};
use crate::usb::KinectUsb;
use log::{debug, info, warn};
use nusb::DeviceInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Open,
    Streaming,
    Closed,
}

pub struct Kinect {
    usb: KinectUsb,
    tx: CommandTransaction,
    state: State,
    serial: String,
    firmware: Option<String>,
}

impl Kinect {
    pub fn open_default_device() -> error::Result<Self> {
        let kinects = KinectUsb::list_devices()?;
        info!("found {} Kinect(s)", kinects.len());
        let kinect_info = kinects.first().ok_or(Error::NoDevice)?;
        Self::open(kinect_info)
    }

    pub fn open(info: &DeviceInfo) -> error::Result<Self> {
        let usb = KinectUsb::open(info)?;
        let tx = CommandTransaction::new(usb.control_interface())?;
        let usb_serial = info.serial_number().unwrap_or("unknown").to_owned();

        Ok(Self {
            usb,
            tx,
            state: State::Open,
            serial: usb_serial,
            firmware: None,
        })
    }

    pub fn start_rgb(&mut self) -> error::Result<()> {
        self.usb.set_video_function_enabled(true)?;

        let firmware_bytes = self.tx.execute(&Command::read_firmware_versions())?;
        self.firmware = response::parse_firmware(&firmware_bytes);
        info!(
            "firmware: {}",
            self.firmware.as_deref().unwrap_or("unknown")
        );

        let serial_bytes = self.tx.execute(&Command::read_serial_number())?;
        let new_serial = response::parse_serial(&serial_bytes);
        info!("serial number: {new_serial}");

        if new_serial != self.serial {
            warn!(
                "serial number reported by nusb {} differs \
                from serial number {} in device protocol",
                self.serial, new_serial
            );
        }

        Ok(())
    }

    fn stop(&mut self) {
        todo!()
    }

    fn close(&mut self) {
        todo!()
    }
}

impl Drop for Kinect {
    fn drop(&mut self) {
        debug!("drop Kinect")
    }
}
