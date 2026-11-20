//! Window and rendering on the main thread, with SDL2.
//!
//! One loop: handle events, upload a new frame if one is waiting, draw.
//! SDL's renderer scales the texture to the window on the GPU.
use std::sync::mpsc::Receiver;

use log::info;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::Texture;

use crate::decode::Frame;

/// Opens the window and blocks until it's closed or Escape is pressed.
/// Must run on the main thread.
pub(crate) fn run(frames: Receiver<Frame>) -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    // Linear filtering when the texture is scaled to the window size.
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");

    let window = video
        .window("Kinect RGB - ESC to exit", 960, 540)
        .position_centered()
        .resizable()
        .build()
        .map_err(|e| e.to_string())?;
    // A GPU-backed renderer; present() waits for vsync, which paces the loop.
    let mut canvas = window
        .into_canvas()
        .accelerated()
        .present_vsync()
        .build()
        .map_err(|e| e.to_string())?;
    let texture_creator = canvas.texture_creator();
    // Created on the first frame, once the size is known, then reused.
    let mut texture: Option<Texture> = None;
    let mut events = sdl.event_pump()?;

    'running: loop {
        // Handle every pending event (close button, Escape, resize).
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::ESCAPE),
                    ..
                } => break 'running,
                _ => {}
            }
        }

        // Never block: take a frame only if one is waiting.
        if let Ok(frame) = frames.try_recv() {
            if texture.is_none() {
                let (w, h) = (frame.width as u32, frame.height as u32);
                texture = Some(
                    texture_creator
                        .create_texture_streaming(PixelFormatEnum::RGB24, w, h)
                        .map_err(|e| e.to_string())?,
                );
                // Draw in 1920x1080 coordinates; SDL letterboxes to keep 16:9.
                canvas.set_logical_size(w, h).map_err(|e| e.to_string())?;
                info!("texture created: {w}x{h}");
            }
            // nvJPEG's RGB bytes go straight in. Pitch = bytes per row.
            texture
                .as_mut()
                .expect("created above")
                .update(None, &frame.data, frame.width * 3)
                .map_err(|e| e.to_string())?;
            // `frame` drops here; SDL has its own copy of the pixels.
        }

        canvas.clear();
        if let Some(texture) = &texture {
            canvas.copy(texture, None, None)?; // None, None = whole texture, whole window
        }
        canvas.present();
    }
    Ok(())
}
