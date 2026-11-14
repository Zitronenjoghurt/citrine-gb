use citrine_gb::nds::geometry::frame::{Frame, MAX_POLYGON_VERTICES, Polygon, Vertex};
use citrine_gb::nds::render::texture::{Decoded, TextureMemory};
use citrine_gb::nds::render::{
    ALPHA_TEST_REF, CLEAR_COLOR, CLEAR_DEPTH, DISP3DCNT, EDGE_COLOR, FOG_COLOR, FOG_OFFSET,
    FOG_TABLE, Pixel, Registers, Renderer, TOON_TABLE,
};
use citrine_gb::nds::video::{HEIGHT, Line3d, WIDTH};
use melonds_sys as sys;
use std::path::Path;

const MAGIC: &[u8; 8] = b"C3DSCENE";
const FINAL_PASS: u32 = (1 << 4) | (1 << 5) | (1 << 7);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Final,
    Plain,
}

pub struct Scene {
    raw: Box<sys::Scene3d>,
}

pub struct Memory<'a> {
    texture: &'a [u8],
    palette: &'a [u8],
    decoded: Decoded,
}

impl Memory<'_> {
    fn prepare(&mut self, frame: &Frame) {
        let mut decoded = std::mem::take(&mut self.decoded);
        for polygon in &frame.polygons {
            decoded.prepare(self, polygon.texture, polygon.palette);
        }
        self.decoded = decoded;
    }
}

impl TextureMemory for Memory<'_> {
    fn texture(&self, address: u32) -> u8 {
        self.texture[address as usize % sys::TEXTURE_SIZE]
    }

    fn texture_palette(&self, address: u32) -> u8 {
        self.palette[address as usize % sys::PALETTE_SIZE]
    }

    fn decoded(&self, param: u32, palette: u16) -> Option<&[u16]> {
        self.decoded.get(param, palette)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Difference {
    pub pixels: usize,
    pub alpha: usize,
    pub first: Option<(usize, usize)>,
}

impl Scene {
    pub fn zeroed() -> Self {
        let layout = std::alloc::Layout::new::<sys::Scene3d>();
        let raw = unsafe {
            let pointer = std::alloc::alloc_zeroed(layout).cast::<sys::Scene3d>();
            if pointer.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            Box::from_raw(pointer)
        };
        Self { raw }
    }

    pub fn raw(&self) -> &sys::Scene3d {
        &self.raw
    }

    pub fn raw_mut(&mut self) -> &mut sys::Scene3d {
        &mut self.raw
    }

    fn bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                (&*self.raw as *const sys::Scene3d).cast::<u8>(),
                size_of::<sys::Scene3d>(),
            )
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MAGIC.len() + size_of::<sys::Scene3d>());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(self.bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let body = bytes
            .strip_prefix(MAGIC.as_slice())
            .ok_or_else(|| anyhow::anyhow!("not a 3D scene dump"))?;
        anyhow::ensure!(
            body.len() == size_of::<sys::Scene3d>(),
            "a 3D scene dump has {} bytes after its header, not {}",
            size_of::<sys::Scene3d>(),
            body.len()
        );
        let mut scene = Self::zeroed();
        unsafe {
            std::ptr::copy_nonoverlapping(
                body.as_ptr(),
                (&mut *scene.raw as *mut sys::Scene3d).cast::<u8>(),
                body.len(),
            );
        }
        anyhow::ensure!(
            scene.raw.polygon_count as usize <= sys::SCENE_POLYGONS
                && scene.raw.vertex_count as usize <= sys::SCENE_VERTICES,
            "a 3D scene dump with {} polygons and {} vertices",
            scene.raw.polygon_count,
            scene.raw.vertex_count
        );
        Ok(scene)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, self.encode())?;
        Ok(())
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Self::decode(&std::fs::read(path)?)
    }

    pub fn registers(&self, layer: Layer) -> Registers {
        let disp3dcnt = match layer {
            Layer::Final => self.raw.disp3dcnt,
            Layer::Plain => self.raw.disp3dcnt & !FINAL_PASS,
        };
        let mut registers = Registers::default();
        for (offset, value) in [
            (DISP3DCNT, disp3dcnt),
            (ALPHA_TEST_REF, self.raw.alpha_reference),
            (CLEAR_COLOR, self.raw.clear_color),
            (CLEAR_DEPTH, self.raw.clear_depth),
            (FOG_COLOR, self.raw.fog_color),
            (FOG_OFFSET, self.raw.fog_offset),
        ] {
            registers.write(offset, value, u32::MAX);
        }
        let pairs = |table: &[u32]| {
            table
                .chunks(2)
                .map(|pair| pair[0] | pair[1] << 16)
                .collect::<Vec<_>>()
        };
        for (index, word) in pairs(&self.raw.edge).into_iter().enumerate() {
            registers.write(EDGE_COLOR + index * 4, word, u32::MAX);
        }
        for (index, word) in pairs(&self.raw.toon).into_iter().enumerate() {
            registers.write(TOON_TABLE + index * 4, word, u32::MAX);
        }
        for (index, bytes) in self.raw.fog_density.chunks(4).enumerate() {
            let word = bytes.iter().enumerate().fold(0, |word, (byte, &value)| {
                word | (value & 0xFF) << (byte * 8)
            });
            registers.write(FOG_TABLE + index * 4, word, u32::MAX);
        }
        registers
    }

    pub fn memory(&self) -> Memory<'_> {
        Memory {
            texture: &self.raw.texture,
            palette: &self.raw.palette,
            decoded: Decoded::default(),
        }
    }

    pub fn frame(&self) -> Frame {
        let raw = &*self.raw;
        let vertices = raw.vertices[..raw.vertex_count as usize]
            .iter()
            .map(vertex)
            .collect();
        let polygons = raw.polygons[..raw.polygon_count as usize]
            .iter()
            .map(polygon)
            .collect();
        Frame { polygons, vertices }
    }

    pub fn melonds(&self, layer: Layer, x: usize, y: usize) -> Pixel {
        let pixels = match layer {
            Layer::Final => &self.raw.layer,
            Layer::Plain => &self.raw.plain,
        };
        let [r, g, b, alpha] = pixels[y * WIDTH + x].to_le_bytes();
        Pixel {
            r: r & 0x3F,
            g: g & 0x3F,
            b: b & 0x3F,
            alpha: alpha & 0x1F,
        }
    }

    pub fn render_cpu(&self, layer: Layer) -> Renderer {
        let frame = self.frame();
        let mut memory = self.memory();
        let registers = self.registers(layer);
        if registers.textured() {
            memory.prepare(&frame);
        }
        let mut renderer = Renderer::default();
        renderer.start_frame(registers);
        let mut line = Line3d::default();
        for y in 0..HEIGHT {
            renderer.render_line(y, &frame, &memory, &mut line);
        }
        renderer
    }

    pub fn difference(&self, layer: Layer, ours: impl Fn(usize, usize) -> Pixel) -> Difference {
        let mut difference = Difference::default();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let (a, b) = (ours(x, y), self.melonds(layer, x, y));
                if !same(a, b) {
                    difference.pixels += 1;
                    difference.alpha += usize::from(a.alpha != b.alpha);
                    difference.first.get_or_insert((x, y));
                }
            }
        }
        difference
    }
}

pub fn same(a: Pixel, b: Pixel) -> bool {
    a.alpha == b.alpha && (a.alpha == 0 || (a.r, a.g, a.b) == (b.r, b.g, b.b))
}

fn vertex(raw: &sys::SceneVertex) -> Vertex {
    Vertex {
        position: raw.position,
        screen: raw.screen.map(|value| value as u16),
        subpixel: raw.hires.map(|value| (value & 0xF) as u8),
        color: raw.color.map(|value| match value >> 4 {
            0 => 0,
            level => (level * 2 + 1) as u8,
        }),
        texcoord: raw.texcoord.map(|value| value as i16),
        clipped: raw.clipped != 0,
    }
}

fn polygon(raw: &sys::ScenePolygon) -> Polygon {
    let mut vertices = [0u16; MAX_POLYGON_VERTICES];
    for (slot, &index) in vertices.iter_mut().zip(&raw.vertices) {
        *slot = index as u16;
    }
    Polygon {
        vertices,
        count: raw.count as u8,
        depth: raw.z,
        w: raw.w,
        attributes: raw.attributes,
        texture: raw.texture,
        palette: raw.palette as u16,
        front: raw.front != 0,
        translucent: raw.translucent != 0,
        w_buffer: raw.w_buffer != 0,
        degenerate: raw.degenerate != 0,
        top: raw.top as u8,
        bottom: raw.bottom as u8,
    }
}

pub fn rgba(pixel: Pixel) -> [u8; 4] {
    let widen = |channel: u8| (channel << 2) | (channel >> 4);
    if pixel.alpha == 0 {
        return [0, 0, 0, 0xFF];
    }
    [widen(pixel.r), widen(pixel.g), widen(pixel.b), 0xFF]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle() -> Scene {
        let mut scene = Scene::zeroed();
        let raw = scene.raw_mut();
        raw.disp3dcnt = 1;
        raw.clear_depth = 0x7FFF;
        for (index, [x, y]) in [[10, 10], [60, 10], [10, 60]].into_iter().enumerate() {
            raw.vertices[index] = sys::SceneVertex {
                position: [0, 0, 0, 0x1000],
                screen: [x, y],
                hires: [x << 4, y << 4],
                color: [(31 << 4) + 0xF, 0, (8 << 4) + 0xF],
                texcoord: [0, 0],
                clipped: 0,
            };
        }
        raw.vertex_count = 3;
        let mut vertices = [0; 10];
        vertices[..3].copy_from_slice(&[0, 1, 2]);
        raw.polygons[0] = sys::ScenePolygon {
            vertices,
            count: 3,
            z: [0x1000; 10],
            w: [0x1000; 10],
            attributes: 31 << 16 | 3 << 6,
            texture: 0,
            palette: 0,
            w_buffer: 0,
            degenerate: 0,
            front: 1,
            translucent: 0,
            top: 0,
            bottom: 2,
        };
        raw.polygon_count = 1;
        scene
    }

    #[test]
    fn a_dump_reads_back_byte_for_byte() {
        let scene = triangle();
        let back = Scene::decode(&scene.encode()).unwrap();
        assert_eq!(back.bytes(), scene.bytes());
        assert_eq!(back.frame(), scene.frame());
    }

    #[test]
    fn a_short_or_foreign_file_is_refused() {
        assert!(Scene::decode(b"C3DSCENE1234").is_err());
        assert!(Scene::decode(b"something else").is_err());
    }

    #[test]
    fn melonds_vertex_colors_become_six_bit_levels() {
        let frame = triangle().frame();
        assert_eq!(frame.vertices[0].color, [63, 0, 17]);
        assert_eq!(frame.vertices[0].subpixel, [0, 0]);
        assert_eq!(frame.polygons[0].vertices(), [0, 1, 2]);
    }

    #[test]
    fn the_cpu_replay_draws_the_triangle() {
        let scene = triangle();
        let renderer = scene.render_cpu(Layer::Final);
        let inside = renderer.pixel(15, 15);
        assert_eq!((inside.r, inside.b, inside.alpha), (63, 17, 31));
        assert_eq!(renderer.pixel(100, 100).alpha, 0);
    }

    #[test]
    fn the_replay_takes_the_edge_table_from_the_dump() {
        let mut scene = triangle();
        let raw = scene.raw_mut();
        raw.disp3dcnt = 1 | 1 << 5;
        raw.polygons[0].attributes |= 9 << 24;
        raw.edge[1] = 31 << 10;
        let renderer = scene.render_cpu(Layer::Final);
        let edge = renderer.pixel(9, 30);
        assert_eq!((edge.r, edge.g, edge.b), (0, 0, 63));
        let inside = renderer.pixel(10, 30);
        assert_eq!((inside.r, inside.b), (63, 17));
        let plain = scene.render_cpu(Layer::Plain).pixel(10, 30);
        assert_eq!((plain.r, plain.b), (63, 17));
    }

    #[test]
    fn transparent_pixels_compare_by_alpha_alone() {
        let blank = Pixel::default();
        let tinted = Pixel {
            r: 5,
            ..Pixel::default()
        };
        assert!(same(blank, tinted));
        assert!(!same(
            Pixel { alpha: 31, ..blank },
            Pixel {
                alpha: 31,
                ..tinted
            }
        ));
    }
}
