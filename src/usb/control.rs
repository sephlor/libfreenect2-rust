use log::debug;
use nusb::MaybeFuture;
use nusb::transfer::{ControlOut, ControlType, Recipient};

use super::{KinectUsb, TIMEOUT};
use crate::error;

impl KinectUsb {
    pub(super) fn init_control_settings(&self) -> error::Result<()> {
        debug!("setting isochronous delay");
        self.set_isochronous_delay()?;

        debug!("disabling IR interface");
        self.set_ir_interface_enabled(false)?;

        debug!("enabling power states");
        self.enable_power_states()?;

        debug!("suspending video function");
        self.set_video_function_enabled(false)?;
        Ok(())
    }

    fn set_isochronous_delay(&self) -> error::Result<()> {
        const SET_ISOCH_DELAY: u8 = 0x31;
        const ISOCH_DELAY_NS: u16 = 40;
        self.standard_control_out(Recipient::Device, SET_ISOCH_DELAY, ISOCH_DELAY_NS, 0)
    }

    fn enable_power_states(&self) -> error::Result<()> {
        const U1_ENABLE: u16 = 48;
        const U2_ENABLE: u16 = 49;

        self.set_feature(Recipient::Device, U1_ENABLE, 0)?;
        self.set_feature(Recipient::Device, U2_ENABLE, 0)
    }

    pub(crate) fn set_video_function_enabled(&self, enabled: bool) -> error::Result<()> {
        const FUNCTION_SUSPEND: u16 = 0;
        const LOW_POWER_SUSPEND: u16 = 1 << 0;
        const REMOTE_WAKE: u16 = 1 << 1;

        let suspend_options = if enabled {
            0
        } else {
            LOW_POWER_SUSPEND | REMOTE_WAKE
        };
        let index = (suspend_options << 8) | u16::from(Self::CONTROL_AND_RGB_INTERFACE);

        self.set_feature(Recipient::Interface, FUNCTION_SUSPEND, index)
    }

    pub(crate) fn set_ir_interface_enabled(&self, enabled: bool) -> error::Result<()> {
        let alt = u8::from(enabled);
        self.ir_interface().set_alt_setting(alt).wait()?;
        Ok(())
    }

    fn set_feature(&self, recipient: Recipient, feature: u16, index: u16) -> error::Result<()> {
        const SET_FEATURE: u8 = 0x03;
        self.standard_control_out(recipient, SET_FEATURE, feature, index)
    }

    fn standard_control_out(
        &self,
        recipient: Recipient,
        request: u8,
        value: u16,
        index: u16,
    ) -> error::Result<()> {
        self.device()
            .control_out(
                ControlOut {
                    control_type: ControlType::Standard,
                    recipient,
                    request,
                    value,
                    index,
                    data: &[],
                },
                TIMEOUT,
            )
            .wait()?;
        Ok(())
    }
}
