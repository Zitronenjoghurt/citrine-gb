#![allow(dead_code)]

pub mod scenes;

use citrine_gb::nds::geometry::Geometry;
use citrine_gb::nds::geometry::command::*;
use citrine_gb::nds::geometry::frame::Frame;
use citrine_gb::nds::render::texture::{PALETTE_SIZE, TEXTURE_SIZE, TextureMemory};
use citrine_gb::nds::render::{
    ALPHA_TEST_REF, CLEAR_COLOR, CLEAR_DEPTH, DISP3DCNT, EDGE_COLOR, FOG_COLOR, FOG_OFFSET,
    FOG_TABLE, Pixel, Registers, Renderer as CpuRenderer,
};
use citrine_gb::nds::video::{HEIGHT, Line3d, WIDTH};
use citrine_gb_nds_gpu::{Antialiasing, Picture, Renderer, Upscale};

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
}

impl Gpu {
    pub fn open() -> Option<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
        let opened = adapter.ok().and_then(|adapter| {
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
        });
        match opened {
            Some((device, queue)) => {
                let renderer = Renderer::new(&device);
                Some(Self {
                    device,
                    queue,
                    renderer,
                })
            }
            None if std::env::var_os("CITRINE_GPU_REQUIRED").is_some() => {
                panic!("no GPU adapter, and CITRINE_GPU_REQUIRED is set")
            }
            None => {
                eprintln!("no GPU adapter, skipped");
                None
            }
        }
    }

    pub fn render(&mut self, scene: &Scene, scale: u32) -> Picture {
        self.render_with(
            scene,
            Upscale {
                scale,
                antialiasing: Antialiasing::Off,
                ..Upscale::default()
            },
        )
    }

    pub fn render_with(&mut self, scene: &Scene, upscale: Upscale) -> Picture {
        self.renderer.render(
            &self.device,
            &self.queue,
            &scene.frame,
            &scene.registers,
            &scene.memory,
            upscale,
        );
        self.renderer.read(&self.device, &self.queue)
    }
}

pub struct Scene {
    pub name: &'static str,
    pub frame: Frame,
    pub registers: Registers,
    pub memory: Memory,
}

impl Scene {
    pub fn cpu(&self) -> CpuRenderer {
        let mut renderer = CpuRenderer::default();
        renderer.start_frame(self.registers);
        let mut line = Line3d::default();
        for y in 0..HEIGHT {
            renderer.render_line(y, &self.frame, &self.memory, &mut line);
        }
        renderer
    }
}

pub struct Memory {
    pub texture: Vec<u8>,
    pub palette: Vec<u8>,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            texture: vec![0; TEXTURE_SIZE as usize],
            palette: vec![0; PALETTE_SIZE as usize],
        }
    }
}

impl Memory {
    pub fn set_color(&mut self, address: u32, color: u16) {
        let at = address as usize;
        self.palette[at..at + 2].copy_from_slice(&color.to_le_bytes());
    }

    pub fn set_texture_u16(&mut self, address: u32, value: u16) {
        let at = address as usize;
        self.texture[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
}

impl TextureMemory for Memory {
    fn texture(&self, address: u32) -> u8 {
        self.texture[(address % TEXTURE_SIZE) as usize]
    }

    fn texture_palette(&self, address: u32) -> u8 {
        self.palette[(address % PALETTE_SIZE) as usize]
    }

    fn texture_bytes(&self) -> Option<(&[u8], &[u8])> {
        Some((&self.texture, &self.palette))
    }
}

pub fn rgb(r: u16, g: u16, b: u16) -> u16 {
    r | g << 5 | b << 10
}

pub struct Gx {
    geometry: Geometry,
}

pub const TRIANGLES: u32 = 0;
pub const QUADS: u32 = 1;
pub const TRIANGLE_STRIP: u32 = 2;
pub const QUAD_STRIP: u32 = 3;

pub const FRONT: u32 = 1 << 7;
pub const BACK: u32 = 1 << 6;
pub const BOTH: u32 = FRONT | BACK;
pub const TRANSLUCENT_DEPTH: u32 = 1 << 11;
pub const FAR_PLANE: u32 = 1 << 12;
pub const DEPTH_EQUAL: u32 = 1 << 14;
pub const FOG: u32 = 1 << 15;
pub const MODULATE: u32 = 0;
pub const DECAL: u32 = 1 << 4;
pub const SHADOW: u32 = 3 << 4;

pub fn alpha(alpha: u32) -> u32 {
    alpha << 16
}

pub fn id(id: u32) -> u32 {
    id << 24
}

fn fixed(value: f32) -> u32 {
    ((value * 4096.0).round() as i32) as u32
}

impl Default for Gx {
    fn default() -> Self {
        let mut gx = Self {
            geometry: Geometry::default(),
        };
        gx.send(VIEWPORT, 0xBFFF_0000);
        gx
    }
}

impl Gx {
    pub fn send(&mut self, command: u8, param: u32) {
        self.geometry.write(usize::from(command) << 2, param, !0);
        self.geometry.step(u32::MAX);
    }

    pub fn perspective(&mut self, near: f32, far: f32) {
        let (sx, sy) = (1.3, 1.3 * 256.0 / 192.0);
        let matrix = [
            sx,
            0.0,
            0.0,
            0.0,
            0.0,
            sy,
            0.0,
            0.0,
            0.0,
            0.0,
            (far + near) / (near - far),
            -1.0,
            0.0,
            0.0,
            2.0 * far * near / (near - far),
            0.0,
        ];
        self.send(MTX_MODE, 0);
        for value in matrix {
            self.send(MTX_LOAD_4X4, fixed(value));
        }
        self.send(MTX_MODE, 2);
        self.send(MTX_IDENTITY, 0);
    }

    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        for value in [x, y, z] {
            self.send(MTX_TRANS, fixed(value));
        }
    }

    pub fn rotate_y(&mut self, degrees: f32) {
        let (s, c) = degrees.to_radians().sin_cos();
        for value in [c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c] {
            self.send(MTX_MULT_3X3, fixed(value));
        }
    }

    pub fn rotate_x(&mut self, degrees: f32) {
        let (s, c) = degrees.to_radians().sin_cos();
        for value in [1.0, 0.0, 0.0, 0.0, c, s, 0.0, -s, c] {
            self.send(MTX_MULT_3X3, fixed(value));
        }
    }

    pub fn push(&mut self) {
        self.send(MTX_PUSH, 0);
    }

    pub fn pop(&mut self) {
        self.send(MTX_POP, 1);
    }

    pub fn attributes(&mut self, attributes: u32) {
        self.send(POLYGON_ATTR, attributes);
    }

    pub fn texture(&mut self, param: u32, palette: u32) {
        self.send(TEXIMAGE_PARAM, param);
        self.send(PLTT_BASE, palette);
    }

    pub fn begin(&mut self, primitive: u32) {
        self.send(BEGIN_VTXS, primitive);
    }

    pub fn end(&mut self) {
        self.send(END_VTXS, 0);
    }

    pub fn color(&mut self, color: u16) {
        self.send(COLOR, u32::from(color));
    }

    pub fn texcoord(&mut self, s: f32, t: f32) {
        let pack = |value: f32| u32::from((value * 16.0).round() as i16 as u16);
        self.send(TEXCOORD, pack(s) | pack(t) << 16);
    }

    pub fn vertex(&mut self, x: f32, y: f32, z: f32) {
        let pack = |value: f32| u32::from((value * 4096.0).round() as i16 as u16);
        self.send(VTX_16, pack(x) | pack(y) << 16);
        self.send(VTX_16, pack(z));
    }

    pub fn finish(mut self, w_buffer: bool, manual: bool) -> Frame {
        self.send(SWAP_BUFFERS, u32::from(manual) | u32::from(w_buffer) << 1);
        self.geometry.vblank();
        self.geometry.with_frame(Clone::clone)
    }
}

pub fn texture_param(format: u32, offset: u32, size_code: u32, repeat: bool, flip: bool) -> u32 {
    let repeat = if repeat { 3 << 16 } else { 0 };
    let flip = if flip { 3 << 18 } else { 0 };
    offset >> 3 | repeat | flip | size_code << 20 | size_code << 23 | format << 26
}

pub fn registers(disp3dcnt: u32, clear_color: u32, clear_depth: u32, alpha_ref: u32) -> Registers {
    let mut registers = Registers::default();
    registers.write(DISP3DCNT, disp3dcnt, !0);
    registers.write(CLEAR_COLOR, clear_color, !0);
    registers.write(CLEAR_DEPTH, clear_depth, !0);
    registers.write(ALPHA_TEST_REF, alpha_ref, !0);
    registers
}

pub struct Mismatch {
    pub count: usize,
    pub first: Vec<String>,
}

pub fn compare_exact(cpu: &CpuRenderer, gpu: &Picture) -> Mismatch {
    let mut mismatch = Mismatch {
        count: 0,
        first: Vec::new(),
    };
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let attribute = cpu.attribute(x, y).with_coverage(0);
            let expected = (cpu.pixel(x, y), cpu.depth(x, y), attribute);
            let actual = (gpu.pixel(x, y), gpu.depth(x, y), gpu.attribute(x, y));
            if expected != actual {
                mismatch.count += 1;
                if mismatch.first.len() < 5 {
                    mismatch
                        .first
                        .push(format!("({x}, {y}): cpu {expected:?} gpu {actual:?}"));
                }
            }
        }
    }
    mismatch
}

pub fn rgba(pixel: Pixel) -> [u8; 4] {
    let level = |value: u8| (u32::from(value) * 255 / 63) as u8;
    [level(pixel.r), level(pixel.g), level(pixel.b), 255]
}

pub fn save_png(path: &std::path::Path, width: usize, height: usize, pixels: &[[u8; 4]]) {
    let file = std::fs::File::create(path).expect("create the picture");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write the header");
    let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
    writer.write_image_data(&bytes).expect("write the picture");
}

pub fn cpu_pixels(cpu: &CpuRenderer) -> Vec<[u8; 4]> {
    (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .map(|(x, y)| rgba(cpu.pixel(x, y)))
        .collect()
}

pub fn gpu_pixels(picture: &Picture) -> Vec<[u8; 4]> {
    (0..picture.height())
        .flat_map(|y| (0..picture.width()).map(move |x| (x, y)))
        .map(|(x, y)| rgba(picture.pixel(x, y)))
        .collect()
}
