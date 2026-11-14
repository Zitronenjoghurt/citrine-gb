use eframe::egui_wgpu;
use jni::JNIEnv;
use jni::objects::{JObject, JString};
use jni::sys::{jfloat, jint};
use ndk::native_window::NativeWindow;
use std::sync::{LazyLock, Mutex, MutexGuard};
use wgpu::rwh::HasWindowHandle;

pub struct Shown {
    pub window: NativeWindow,
    pub width: u32,
    pub height: u32,
    pub density: f32,
}

#[derive(Default)]
struct Shared {
    name: Option<String>,
    window: Option<Option<Shown>>,
    touches: Vec<(i32, f32, f32)>,
}

static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(Mutex::default);

fn shared() -> MutexGuard<'static, Shared> {
    SHARED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn available() -> Option<String> {
    shared().name.clone()
}

pub fn open() {
    super::java(|env, activity| {
        env.call_method(activity, "openSecondDisplay", "()V", &[])
            .map(drop)
    });
}

pub fn close() {
    super::java(|env, activity| {
        env.call_method(activity, "closeSecondDisplay", "()V", &[])
            .map(drop)
    });
}

pub fn take_window() -> Option<Option<Shown>> {
    shared().window.take()
}

pub fn take_touches() -> Vec<(i32, f32, f32)> {
    std::mem::take(&mut shared().touches)
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeSecondDisplay(
    mut env: JNIEnv,
    _activity: JObject,
    name: JString,
) {
    let name = if name.is_null() {
        None
    } else {
        env.get_string(&name).ok().map(String::from)
    };
    shared().name = name;
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeSecondSurface(
    env: JNIEnv,
    _activity: JObject,
    surface: JObject,
    width: jint,
    height: jint,
    density: jfloat,
) {
    let shown = if surface.is_null() {
        None
    } else {
        unsafe { NativeWindow::from_surface(env.get_raw().cast(), surface.as_raw().cast()) }.map(
            |window| Shown {
                window,
                width: u32::try_from(width).unwrap_or(1).max(1),
                height: u32::try_from(height).unwrap_or(1).max(1),
                density: density.max(1.0),
            },
        )
    };
    shared().window = Some(shown);
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeSecondTouch(
    _env: JNIEnv,
    _activity: JObject,
    action: jint,
    x: jfloat,
    y: jfloat,
) {
    shared().touches.push((action, x, y));
    super::repaint();
}

pub struct Painter {
    _window: NativeWindow,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: egui_wgpu::Renderer,
    pub ctx: egui::Context,
    density: f32,
    down: bool,
}

const DOWN: i32 = 0;
const UP: i32 = 1;
const CANCEL: i32 = 3;

impl Painter {
    pub fn new(shown: Shown) -> Option<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let handle = shown.window.window_handle().ok()?.as_raw();
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: wgpu::rwh::RawDisplayHandle::Android(
                wgpu::rwh::AndroidDisplayHandle::new(),
            ),
            raw_window_handle: handle,
        };
        let surface = unsafe { instance.create_surface_unsafe(target) }.ok()?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: shown.width,
            height: shown.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes.first().copied()?,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        Some(Self {
            _window: shown.window,
            surface,
            device,
            queue,
            config,
            renderer,
            ctx: egui::Context::default(),
            density: shown.density,
            down: false,
        })
    }

    fn input(&mut self) -> egui::RawInput {
        let size = egui::vec2(self.config.width as f32, self.config.height as f32) / self.density;
        let mut events = Vec::new();
        for (action, x, y) in take_touches() {
            let pos = egui::pos2(x / self.density, y / self.density);
            events.push(egui::Event::PointerMoved(pos));
            let pressed = match action {
                DOWN => Some(true),
                UP | CANCEL => Some(false),
                _ => None,
            };
            if let Some(pressed) = pressed.filter(|p| *p != self.down) {
                self.down = pressed;
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            if action == UP || action == CANCEL {
                events.push(egui::Event::PointerGone);
            }
        }
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        }
    }

    pub fn paint(&mut self, run: impl FnMut(&egui::Context)) {
        let input = self.input();
        self.ctx.set_pixels_per_point(self.density);
        let output = self.ctx.run(input, run);
        let shapes = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        for (id, delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let commands =
            self.renderer
                .update_buffers(&self.device, &self.queue, &mut encoder, &shapes, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("second display"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &shapes, &screen);
        }
        self.queue
            .submit(commands.into_iter().chain([encoder.finish()]));
        frame.present();
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
