//! Window and rendering on the main thread: winit for the window and events,
//! glutin for the GL context, glow for GL calls.
//!
//! winit owns the loop: `run_app` takes over the main thread and calls the
//! `ApplicationHandler` methods on `App` as events arrive.
use std::error::Error;
use std::num::NonZeroU32;

use glow::HasContext;
use glutin::config::{Api, ConfigTemplateBuilder};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentGlContext, PossiblyCurrentContext,
    Version,
};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use log::{debug, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowId};

use libfreenect2_rust::{Frame, FrameReceiver};

/// Covers the viewport with one oversized triangle, so no vertex buffer is
/// needed. V is flipped because JPEG rows start at the top and GL's start at
/// the bottom.
const VERTEX_SHADER: &str = r#"#version 330 core
out vec2 uv;

void main() {
    vec2 position = vec2(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? 3.0 : -1.0);
    uv = vec2((position.x + 1.0) / 2.0, 1.0 - (position.y + 1.0) / 2.0);
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;

const FRAGMENT_SHADER: &str = r#"#version 330 core
in vec2 uv;
out vec4 color;
uniform sampler2D image;

void main() {
    color = vec4(texture(image, uv).rgb, 1.0);
}
"#;

const RGB_BYTES_PER_PIXEL: usize = 3;

struct App {
    renderer: Option<Renderer>,
    frames: FrameReceiver,
}

struct Renderer {
    gl: glow::Context,
    program: glow::Program,
    // Core profile requires a bound VAO to draw, even with no vertex data.
    vertex_array: glow::VertexArray,
    texture: glow::Texture,
    /// Allocated texture size, `None` until the first frame arrives.
    image_size: Option<(i32, i32)>,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    // Drops last
    window: Window,
}

impl Renderer {
    fn new(event_loop: &ActiveEventLoop) -> Self {
        let attributes = Window::default_attributes()
            .with_title("Kinect RGB - ESC to exit")
            .with_inner_size(LogicalSize::new(960.0, 540.0));

        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attributes))
            .build(
                event_loop,
                ConfigTemplateBuilder::new().with_api(Api::OPENGL),
                |mut configs| configs.next().expect("no GL configs"),
            )
            .expect("failed to create GL display");
        let window = window.expect("window wasn't created");
        let display = config.display();

        let raw_handle = window.window_handle().ok().map(|h| h.as_raw());
        let context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .with_profile(GlProfile::Core)
            .build(raw_handle);
        // SAFETY: context outlives the window
        let context = unsafe { display.create_context(&config, &context_attributes) }
            .expect("failed to create GL context");

        let surface_attributes = window
            .build_surface_attributes(Default::default())
            .expect("failed to get surface attributes");
        // SAFETY: as above, the window outlives the surface.
        let surface = unsafe { display.create_window_surface(&config, &surface_attributes) }
            .expect("failed to create GL surface");

        let context = context
            .make_current(&surface)
            .expect("failed to make GL context current");

        // Vsync: swap_buffers waits for the next display refresh.
        let one = NonZeroU32::new(1).expect("1 is not zero");
        if let Err(e) = surface.set_swap_interval(&context, SwapInterval::Wait(one)) {
            warn!("couldn't enable vsync: {e}");
        }

        // SAFETY: the context is current, so the loaded pointers belong to it.
        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name))
        };
        // SAFETY: a plain query with a current context.
        let version = unsafe { gl.get_parameter_string(glow::VERSION) };
        info!("window opened, GL {version}");

        // SAFETY: the context is current on this thread.
        let (program, vertex_array, texture) = unsafe {
            let program = create_program(&gl);
            let vertex_array = gl
                .create_vertex_array()
                .expect("failed to create vertex array");
            let texture = gl.create_texture().expect("failed to create texture");

            gl.use_program(Some(program));
            gl.bind_vertex_array(Some(vertex_array));
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            for (parameter, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, parameter, value as i32);
            }
            // RGB rows aren't always a multiple of GL's default 4-byte alignment.
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);

            (program, vertex_array, texture)
        };

        Self {
            gl,
            program,
            vertex_array,
            texture,
            image_size: None,
            surface,
            context,
            window,
        }
    }

    fn upload(&mut self, frame: &Frame) {
        let (width, height, data, sequence) = (
            frame.width(),
            frame.height(),
            frame.data(),
            frame.sequence(),
        );
        let expected = width * height * RGB_BYTES_PER_PIXEL;
        if data.len() != expected {
            warn!(
                "seq {sequence}: expected {expected} bytes, got {}",
                data.len()
            );
            return;
        }
        let (Ok(width), Ok(height)) = (i32::try_from(width), i32::try_from(height)) else {
            warn!("seq {}: frame too large: {}x{}", sequence, width, height);
            return;
        };

        // SAFETY: the context is current, the texture is bound (in `new`), and
        // `data` was checked above to hold exactly width * height RGB pixels.
        unsafe {
            if self.image_size != Some((width, height)) {
                self.gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGB8 as i32,
                    width,
                    height,
                    0,
                    glow::RGB,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                self.image_size = Some((width, height));
            }
            self.gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                width,
                height,
                glow::RGB,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(data)),
            );
        }
    }

    fn resize(&self, width: u32, height: u32) {
        if let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            self.surface.resize(&self.context, width, height);
        }
    }

    fn draw(&self) {
        let window_size = self.window.inner_size();
        // SAFETY: the context is current on this thread (made so in `new`),
        // and the program, VAO, and texture stay bound.
        unsafe {
            self.gl.clear(glow::COLOR_BUFFER_BIT);
            if let Some(image_size) = self.image_size {
                let (x, y, width, height) = letterbox(
                    (window_size.width as i32, window_size.height as i32),
                    image_size,
                );
                self.gl.viewport(x, y, width, height);
                self.gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }
        }
        // Show what we drew. With vsync this blocks until the next refresh,
        // which paces the whole loop.
        self.surface
            .swap_buffers(&self.context)
            .expect("swap_buffers failed");
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // SAFETY: the context is still current, and these objects aren't used
        // again.
        unsafe {
            self.gl.delete_texture(self.texture);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.program);
        }
    }
}

/// Largest centered viewport with the image's aspect ratio, as (x, y, w, h).
fn letterbox(window: (i32, i32), image: (i32, i32)) -> (i32, i32, i32, i32) {
    let (window_w, window_h) = (i64::from(window.0), i64::from(window.1));
    let (image_w, image_h) = (i64::from(image.0), i64::from(image.1));
    let (w, h) = if window_w * image_h > window_h * image_w {
        (window_h * image_w / image_h, window_h)
    } else {
        (window_w, window_w * image_h / image_w)
    };
    (
        ((window_w - w) / 2) as i32,
        ((window_h - h) / 2) as i32,
        w as i32,
        h as i32,
    )
}

/// # Safety
/// A GL context must be current on this thread.
unsafe fn create_program(gl: &glow::Context) -> glow::Program {
    // SAFETY: the caller guarantees a current context.
    unsafe {
        let program = gl.create_program().expect("failed to create GL program");
        let mut shaders = Vec::with_capacity(2);
        for (kind, source) in [
            (glow::VERTEX_SHADER, VERTEX_SHADER),
            (glow::FRAGMENT_SHADER, FRAGMENT_SHADER),
        ] {
            let shader = gl.create_shader(kind).expect("failed to create GL shader");
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            assert!(
                gl.get_shader_compile_status(shader),
                "shader compile failed: {}",
                gl.get_shader_info_log(shader)
            );
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.link_program(program);
        assert!(
            gl.get_program_link_status(program),
            "program link failed: {}",
            gl.get_program_info_log(program)
        );
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        program
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new(event_loop));
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(renderer) = &mut self.renderer else {
            return;
        };
        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                info!("window closed");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => renderer.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                // Take a new frame if one is waiting, without blocking.
                // Otherwise the last uploaded image is drawn again.
                if let Ok(frame) = self.frames.try_recv() {
                    debug!(
                        "seq {}: {}x{}",
                        frame.sequence(),
                        frame.width(),
                        frame.height()
                    );
                    renderer.upload(&frame);
                }
                renderer.draw();
                // Ask for the next redraw right away; vsync in `draw` paces it.
                renderer.window.request_redraw();
            }
            _ => {}
        }
    }
}

/// Opens the window and blocks until it's closed. Must run on the main thread.
pub(crate) fn run(frames: FrameReceiver) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        renderer: None,
        frames,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
