use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle, sleep};
use std::time::Duration;

use log::{debug, error, info, warn};
use nusb::DeviceInfo;

use crate::decode::{self, FrameReceiver};
use crate::error::{self, Error};
use crate::host_memory::HostPool;
use crate::protocol::{Command, CommandTransaction, response};
use crate::rgb::{JpegPacket, RgbStreamParser};
use crate::usb::bulk_stream::BulkStream;
use crate::usb::{self, DeviceHandle};

#[derive(Debug, Clone)]
pub struct KinectInfo {
    serial: String,
}

impl KinectInfo {
    pub fn serial(&self) -> &str {
        &self.serial
    }
}

pub fn list_kinects() -> error::Result<Vec<KinectInfo>> {
    let kinects = usb::find_kinects()?;
    info!("found {} Kinect(s)", kinects.len());
    Ok(kinects
        .iter()
        .map(|info| KinectInfo {
            serial: info
                .serial_number()
                .expect("usb::find_kinects skips devices without a serial")
                .to_owned(),
        })
        .collect())
}

#[derive(Debug)]
pub struct Kinect {
    usb: DeviceHandle,
    tx: CommandTransaction,
    state: State,
    serial: String,
    firmware: Option<String>,
    rgb_stream: Option<BulkStream>,
    decode_thread: Option<JoinHandle<()>>,
    pool: Option<HostPool>,
}

impl Kinect {
    const STATUS_POLL_ATTEMPTS: u8 = 50;
    const STATUS_REGISTER: u32 = 0x090000;
    const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(100);

    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn firmware(&self) -> Option<&str> {
        self.firmware.as_deref()
    }

    pub fn open(serial: &str) -> error::Result<Self> {
        let kinects = usb::find_kinects()?;
        if kinects.is_empty() {
            return Err(Error::NoDevice);
        }
        let kinect_info = kinects
            .iter()
            .find(|dev| dev.serial_number() == Some(serial))
            .ok_or_else(|| Error::DeviceNotFound {
                serial: serial.to_owned(),
            })?;
        Self::open_info(kinect_info)
    }

    pub fn open_default() -> error::Result<Self> {
        let kinects = usb::find_kinects()?;
        info!("found {} Kinect(s)", kinects.len());
        let kinect_info = kinects.first().ok_or(Error::NoDevice)?;
        Self::open_info(kinect_info)
    }

    fn open_info(info: &DeviceInfo) -> error::Result<Self> {
        let serial = info
            .serial_number()
            .ok_or(Error::NoSerialNumber)?
            .to_owned();
        let usb = DeviceHandle::open(info)?;
        let mut tx = CommandTransaction::new(usb.control_and_rgb_interface())?;

        usb.set_video_function_enabled(true)?;
        let firmware_bytes = tx.execute(&Command::read_firmware_versions());
        usb.set_video_function_enabled(false)?;
        let firmware = response::parse_firmware(&firmware_bytes?);
        info!("firmware: {}", firmware.as_deref().unwrap_or("unknown"));

        Ok(Self {
            usb,
            tx,
            state: State::Open,
            serial,
            firmware,
            rgb_stream: None,
            decode_thread: None,
            pool: None,
        })
    }

    pub fn start(&mut self) -> error::Result<FrameReceiver> {
        match self.state {
            State::Closed => return Err(Error::DeviceClosed),
            State::Streaming => return Err(Error::AlreadyStreaming),
            State::Open => {}
        }

        let pool = match &self.pool {
            Some(pool) => pool.clone(),
            None => self.pool.insert(HostPool::new()?).clone(),
        };

        let parser = RgbStreamParser::new(pool);

        // TODO: count drops for both
        let (packet_tx, packet_rx) = mpsc::sync_channel::<JpegPacket>(1);
        let (frame_tx, frame_rx) = mpsc::sync_channel(1);

        #[cfg(feature = "cuda")]
        let decoder = decode::run_gpu;
        #[cfg(not(feature = "cuda"))]
        let decoder = decode::run_cpu;

        let decode_thread = thread::Builder::new()
            .name("decode".into())
            .spawn(move || decoder(packet_rx, frame_tx))?;

        // TODO: pass a closure instead?
        if let Err(e) = self.start_rgb(parser, packet_tx) {
            if decode_thread.join().is_err() {
                error!("decode thread panicked");
            }
            return Err(e);
        }
        self.decode_thread = Some(decode_thread);

        Ok(FrameReceiver::new(frame_rx))
    }

    fn start_rgb(
        &mut self,
        parser: RgbStreamParser,
        packets: SyncSender<JpegPacket>,
    ) -> error::Result<()> {
        info!("starting");

        self.usb.set_video_function_enabled(true)?;

        self.tx.execute(&Command::read_hardware_info())?;

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

        // MAYBE TODO: ReadDepthCameraParametersCommand
        // MAYBE TODO: setIrCameraParams (used for registration?)

        // MAYBE TODO: ReadP0TablesCommand
        // MAYBE TODO: something about getDepthPacketProcessor()->loadP0TablesFromCommandResponse

        self.tx.execute(&Command::read_rgb_camera_params())?;
        // TODO: setColorCameraParams (used for registration?)

        self.tx
            .execute(&Command::set_mode_enabled_with_param(0x00640064))?;
        self.tx.execute(&Command::set_mode_disabled())?;

        let mut ready = false;
        for _ in 0..Self::STATUS_POLL_ATTEMPTS {
            let status_bytes = self
                .tx
                .execute(&Command::read_status(Self::STATUS_REGISTER))?;
            let status = response::parse_status(&status_bytes);
            debug!("status: {:?}", status);
            if status.is_some_and(|s| (s & 1) == 1) {
                ready = true;
                break;
            }
            sleep(Self::STATUS_POLL_INTERVAL);
        }
        if !ready {
            warn!(
                "device status {:#x} not ready after {} attempts, continuing",
                Self::STATUS_REGISTER,
                Self::STATUS_POLL_ATTEMPTS
            );
        }

        self.tx.execute(&Command::init_streams())?;

        self.usb.set_ir_interface_enabled(true)?;

        let status_bytes = self
            .tx
            .execute(&Command::read_status(Self::STATUS_REGISTER))?;
        let status = response::parse_status(&status_bytes);
        debug!("status: {:?}", status);

        self.tx.execute(&Command::set_streaming_enabled())?;
        self.state = State::Streaming;
        info!("streaming enabled");

        let stream = BulkStream::start(self.usb.control_and_rgb_interface(), parser, packets)?;
        self.rgb_stream = Some(stream);

        Ok(())
    }

    pub fn stop(&mut self) -> error::Result<()> {
        if self.state == State::Streaming {
            info!("stopping stream");

            if let Some(stream) = self.rgb_stream.take() {
                stream.stop();
            }
            if let Some(decode_thread) = self.decode_thread.take()
                && decode_thread.join().is_err()
            {
                error!("decode thread panicked");
            }

            self.usb.set_ir_interface_enabled(false)?;

            self.tx
                .execute(&Command::set_mode_enabled_with_param(0x00640064))?;
            self.tx.execute(&Command::set_mode_disabled())?;
            self.tx.execute(&Command::stop())?;
            self.tx.execute(&Command::set_streaming_disabled())?;
            self.tx.execute(&Command::set_mode_enabled())?;
            self.tx.execute(&Command::set_mode_disabled())?;
            self.tx.execute(&Command::set_mode_enabled())?;
            self.tx.execute(&Command::set_mode_disabled())?;

            self.usb.set_video_function_enabled(false)?;

            self.state = State::Open;
            info!("streaming stopped");
        } else {
            info!("already stopped, doing nothing");
        }
        Ok(())
    }

    pub fn close(mut self) -> error::Result<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> error::Result<()> {
        match self.state {
            State::Closed => {
                debug!("already closed, doing nothing");
                return Ok(());
            }
            State::Streaming => {
                if let Err(e) = self.stop() {
                    warn!("stop failed while closing, continuing: {e}");
                }
            }
            State::Open => {}
        }
        info!("closing device");

        let mode_on = self
            .tx
            .execute(&Command::set_mode_enabled_with_param(0x00640064));
        let mode_off = self.tx.execute(&Command::set_mode_disabled());

        // MAYBE TODO: something about getRgbPacketProcessor()->setFrameListener(0)
        // MAYBE TODO: something about getDepthPacketProcessor()->setFrameListener(0)

        // MAYBE TODO: release usb interfaces

        // MAYBE TODO: deallocate rgb transfer pool
        // MAYBE TODO: deallocate ir transfer pool

        // MAYBE TODO: close usb device and handle

        self.state = State::Closed;
        info!("device closed");
        mode_on.and(mode_off).map(|_| ())
    }
}

impl Drop for Kinect {
    fn drop(&mut self) {
        debug!("dropping Kinect");
        if let Err(e) = self.close_inner() {
            error!("closing device failed: {e}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Open,
    Streaming,
    Closed,
}
