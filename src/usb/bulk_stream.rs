use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};
use nusb::transfer::{Bulk, In, TransferError};
use nusb::{Endpoint, Interface};

use crate::error::{self};
use crate::rgb::RgbStreamParser;
use crate::usb::TIMEOUT;

#[derive(Debug)]
pub(crate) struct BulkStream {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl BulkStream {
    const EP_IN: u8 = 0x83;
    // Try tuning these with env variables later. Remember it must be a multiple of endpoint.max_packet_size()
    const XFER_SIZE: usize = 0x4000;
    const NUM_XFERS: usize = 20;

    pub(crate) fn start(interface: &Interface) -> error::Result<Self> {
        let mut endpoint = interface.endpoint::<Bulk, In>(Self::EP_IN)?;
        // mut?
        let parser = RgbStreamParser::new();

        while endpoint.pending() < Self::NUM_XFERS {
            let buffer = endpoint.allocate(Self::XFER_SIZE);
            endpoint.submit(buffer);
        }

        let running = Arc::new(AtomicBool::new(true));

        let handle = thread::Builder::new().name("usb".into()).spawn({
            let running = running.clone();
            move || run(endpoint, parser, running)
        })?;

        Ok(Self {
            running,
            handle: Some(handle),
        })
    }

    pub(crate) fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        self.running.store(false, Ordering::Relaxed);
        if handle.join().is_err() {
            error!("usb thread panicked");
        }
    }
}

impl Drop for BulkStream {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(mut endpoint: Endpoint<Bulk, In>, mut parser: RgbStreamParser, running: Arc<AtomicBool>) {
    let mut bytes = 0;
    let mut window_start = Instant::now();

    while running.load(Ordering::Relaxed) {
        let Some(completion) = endpoint.wait_next_complete(TIMEOUT) else {
            warn!("no data within {TIMEOUT:?}");
            continue;
        };

        match completion.status {
            Ok(()) => {
                parser.parse_packets(&completion.buffer);
                bytes += completion.buffer.len();
            }
            Err(TransferError::Cancelled) => {
                debug!("transfer cancelled, stopping");
                break;
            }
            Err(TransferError::Disconnected) => {
                error!("device disconnected, stopping usb thread");
                break;
            }
            Err(e) => warn!("transfer failed, resubmitting: {e}"),
        }

        let elapsed = window_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let mb_per_s = bytes as f64 / elapsed.as_secs_f64() / 1_000_000.0;
            info!("rgb rate: {mb_per_s:.2} MB/s");
            bytes = 0;
            window_start = Instant::now();
        }

        endpoint.submit(completion.buffer);
    }

    drain(&mut endpoint);
}

fn drain(endpoint: &mut Endpoint<Bulk, In>) {
    endpoint.cancel_all();

    while endpoint.pending() > 0 {
        if endpoint.wait_next_complete(TIMEOUT).is_none() {
            warn!(
                "{} transfers still pending after cancel, giving up",
                endpoint.pending()
            );
            return;
        }
    }
    debug!("all transfers returned");
}
