//! configuration 1
//!   interface 0
//!     alt setting 0
//!       endpoint 0x81  Bulk In  max packet 1024
//!       endpoint 0x02  Bulk Out  max packet 1024
//!       endpoint 0x83  Bulk In  max packet 1024
//!       endpoint 0x82  Interrupt In  max packet 64
//!   interface 1
//!     alt setting 0
//!     alt setting 1
//!       endpoint 0x84  Isochronous In  max packet 1024
//!   interface 2
//!     alt setting 0
//!   interface 3
//!     alt setting 0
//!     alt setting 1
//!       endpoint 0x85  Isochronous In  max packet 256

use crate::error::{self, Error};
use log::{debug, info, trace, warn};
use nusb::{Device, DeviceInfo, Interface, MaybeFuture};
use std::{thread::sleep, time::Duration};

#[derive(Debug)]
pub struct KinectUsb {
    device: Device,
    control_interface: Interface,
    pub(super) ir_interface: Interface,
}

impl KinectUsb {
    const VENDOR_ID: u16 = 0x045E;
    const PRODUCT_ID: u16 = 0x02D8;
    const PRODUCT_ID_PREVIEW: u16 = 0x02C4;

    pub(super) const IR_INTERFACE: u8 = 1;
    pub(super) const CONTROL_AND_RGB_INTERFACE: u8 = 0;
    pub(super) const TIMEOUT: Duration = Duration::from_millis(1000);

    pub(crate) fn control_interface(&self) -> &Interface {
        &self.control_interface
    }

    pub(crate) fn device(&self) -> &Device {
        &self.device
    }

    pub fn list_devices() -> error::Result<Vec<DeviceInfo>> {
        let kinects: Vec<DeviceInfo> = nusb::list_devices()
            .wait()?
            .filter(Self::is_kinect)
            .filter(|dev| {
                if dev.serial_number().is_none() {
                    warn!(
                        "skipping Kinect at bus {} addr {}: no serial number",
                        dev.busnum(),
                        dev.device_address()
                    );
                    return false;
                }
                true
            })
            .collect();

        debug!("found {} Kinect(s)", kinects.len());
        for dev in &kinects {
            debug!(
                "Kinect at bus {} addr {} {:04x}:{:04x} serial {:?}",
                dev.busnum(),
                dev.device_address(),
                dev.vendor_id(),
                dev.product_id(),
                dev.serial_number()
            );
        }

        Ok(kinects)
    }

    fn is_kinect(dev: &DeviceInfo) -> bool {
        dev.vendor_id() == Self::VENDOR_ID
            && (dev.product_id() == Self::PRODUCT_ID
                || dev.product_id() == Self::PRODUCT_ID_PREVIEW)
    }

    // Reset on every open, like libfreenect2 (libfreenect2.cpp:1219).
    // A previous session that ended mid-transaction (crash, Ctrl-C, unread reply)
    // can leave the Kinect ignoring commands: sends on ep 0x02 time out until the
    // device is replugged. A reset is a software replug that clears that state.
    //
    // After a reset the old handle is dead and the device may re-enumerate at a
    // new address (see nusb `Device::reset` docs), so drop it, wait, and find it
    // again by serial number.
    pub fn open(info: &DeviceInfo) -> error::Result<Self> {
        let device = info.open().wait()?;
        info!("resetting Kinect {}", describe(info));
        device.reset().wait()?;

        drop(device);
        sleep(Duration::from_secs(1));
        let new_info = Self::list_devices()?
            .into_iter()
            .find(|d| d.serial_number() == info.serial_number())
            .ok_or(Error::NoDevice)?;

        Self::open_without_reset(&new_info)
    }

    fn open_without_reset(info: &DeviceInfo) -> error::Result<Self> {
        info!("opening Kinect {}", describe(info));
        let device = info.open().wait()?;

        if log::log_enabled!(log::Level::Trace) {
            log_layout(&device);
        }

        // Set configuration value
        let config = device
            .active_configuration()
            .map(|c| c.configuration_value())
            .unwrap_or(0);
        if config != 1 {
            device.set_configuration(1).wait()?;
        }
        // Claim interfaces
        let control_interface = device
            .claim_interface(Self::CONTROL_AND_RGB_INTERFACE)
            .wait()?;
        let ir_interface = device.claim_interface(Self::IR_INTERFACE).wait()?;

        let kinect = KinectUsb {
            device,
            control_interface,
            ir_interface,
        };
        kinect.init_control_settings()?;

        info!("Kinect {} opened", describe(info));
        Ok(kinect)
    }
}

fn describe(info: &DeviceInfo) -> String {
    format!(
        "{} (bus {} addr {})",
        info.serial_number().unwrap_or("unknown"),
        info.busnum(),
        info.device_address()
    )
}

fn log_layout(device: &Device) {
    for config in device.configurations() {
        let c = config.configuration_value();
        trace!("configuration {c}");
        for iface in config.interfaces() {
            let i = iface.interface_number();
            trace!("configuration {c} interface {i}");
            for alt in iface.alt_settings() {
                let a = alt.alternate_setting();
                trace!("configuration {c} interface {i} alt setting {a}");
                for ep in alt.endpoints() {
                    trace!(
                        "configuration {c} interface {i} alt setting {a} endpoint 0x{:02x} {:?} {:?} max packet {}",
                        ep.address(),
                        ep.transfer_type(),
                        ep.direction(),
                        ep.max_packet_size()
                    );
                }
            }
        }
    }
}
