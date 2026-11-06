mod usb;

use usb::KinectUsb;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kinects = KinectUsb::list_devices()?;

    let kinect_info = kinects.first().ok_or("No Kinect devices to open.")?;

    println!("First KinectUsb: {:?}", kinect_info);

    let kinect = KinectUsb::open(kinect_info)?;

    println!("First KinectUsb post setup: {:?}", kinect);

    Ok(())
}
