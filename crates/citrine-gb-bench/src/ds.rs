use crate::digest::{Digest, Fnv};
use crate::manifest::Workload;
use crate::run::{Outcome, checkpoints};
use citrine_gb::console::Touch;
use citrine_gb::nds::geometry::Geometry;
use citrine_gb::nds::geometry::command::*;
use citrine_gb::nds::machine::{Nds, NdsFirmware};
use citrine_gb::nds::render::texture::TextureMemory;
use citrine_gb::nds::render::{CLEAR_COLOR, CLEAR_DEPTH, DISP3DCNT, Registers, Renderer};
use citrine_gb::nds::video::{Engine, EngineKind, EngineVram, HEIGHT, Line3d, Memory, WIDTH};
use citrine_gb::nds::vram::Bank;
use std::time::Instant;

const ALL: u32 = 0xFFFF_FFFF;
const ONE: i32 = 0x1000;
const TEXTURES_AND_BLENDING: u32 = 0b1001;
const QUADS: u32 = 1;
const STRIPS: u32 = 2;
const LINES: u16 = 263;
const START_TIME: i64 = 1_762_000_000;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
}

fn sine(degrees: i32) -> i32 {
    let x = degrees.rem_euclid(360);
    let (x, sign) = if x < 180 { (x, 1) } else { (x - 180, -1) };
    let p = x * (180 - x);
    sign * (4 * p * ONE) / (40_500 - p)
}

fn cosine(degrees: i32) -> i32 {
    sine(degrees + 90)
}

fn fixed_mul(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 12) as i32
}

struct Gx<'a>(&'a mut Geometry);

impl Gx<'_> {
    fn send(&mut self, command: u8, params: &[u32]) {
        let port = usize::from(command) << 2;
        if params.is_empty() {
            self.0.write(port, 0, ALL);
        }
        for &param in params {
            self.0.write(port, param, ALL);
            if self.0.full() {
                self.0.step(u32::MAX);
            }
        }
    }

    fn vertex(&mut self, x: i32, y: i32, z: i32) {
        self.send(
            VTX_16,
            &[(x as u16 as u32) | (y as u16 as u32) << 16, z as u16 as u32],
        );
    }

    fn normal(&mut self, x: i32, y: i32, z: i32) {
        let pack = |v: i32| (v.clamp(-511, 511) as u32) & 0x3FF;
        self.send(NORMAL, &[pack(x) | pack(y) << 10 | pack(z) << 20]);
    }

    fn texcoord(&mut self, s: i32, t: i32) {
        self.send(TEXCOORD, &[(s as u16 as u32) | (t as u16 as u32) << 16]);
    }

    fn projection(&mut self) {
        self.send(MTX_MODE, &[0]);
        let f = 2 * ONE;
        let near = ONE / 10;
        let far = 40 * ONE;
        let a = ((i64::from(far + near) << 12) / i64::from(near - far)) as i32;
        let b = ((2 * i64::from(far) * i64::from(near)) / i64::from(near - far)) as i32;
        let aspect = f * 3 / 4;
        let m = [aspect, 0, 0, 0, 0, f, 0, 0, 0, 0, a, -ONE, 0, 0, b, 0];
        self.send(MTX_LOAD_4X4, &m.map(|v| v as u32));
        self.send(MTX_MODE, &[2]);
        self.send(MTX_IDENTITY, &[]);
        self.send(VIEWPORT, &[255 << 16 | 191 << 24]);
    }

    fn rotate(&mut self, yaw: i32, pitch: i32) {
        let (sy, cy, sp, cp) = (sine(yaw), cosine(yaw), sine(pitch), cosine(pitch));
        let m = [
            cy,
            fixed_mul(sy, sp),
            -fixed_mul(sy, cp),
            0,
            cp,
            sp,
            sy,
            -fixed_mul(cy, sp),
            fixed_mul(cy, cp),
        ];
        self.send(MTX_MULT_3X3, &m.map(|v| v as u32));
    }

    fn lights(&mut self) {
        self.send(
            LIGHT_VECTOR,
            &[(0x100u32 & 0x3FF)
                | ((-0x100i32 as u32) & 0x3FF) << 10
                | ((-0x180i32 as u32) & 0x3FF) << 20],
        );
        self.send(LIGHT_COLOR, &[0x7FFF]);
        self.send(
            LIGHT_VECTOR,
            &[1 << 30 | ((-0x1C0i32 as u32) & 0x3FF) | (0x80 << 20)],
        );
        self.send(LIGHT_COLOR, &[1 << 30 | 0x03DF]);
        self.send(DIF_AMB, &[0x5EF7 | 0x2108 << 16]);
        self.send(SPE_EMI, &[0x1084 | 0x0421 << 16]);
    }

    fn swap(&mut self, manual: bool) {
        self.send(SWAP_BUFFERS, &[u32::from(manual)]);
        self.0.step(u32::MAX);
        self.0.vblank();
    }
}

struct Vram {
    texture: Vec<u8>,
    palette: Vec<u8>,
}

impl Vram {
    fn new(seed: u64) -> Self {
        let mut rng = Rng(seed | 1);
        let mut texture = vec![0; 0x8_0000];
        for (index, byte) in texture.iter_mut().enumerate().take(0x4000) {
            let cell = ((index >> 4) ^ (index >> 9)) & 1;
            *byte = if cell == 0 {
                rng.next() as u8
            } else {
                (index as u8).wrapping_mul(37)
            };
        }
        let mut palette = vec![0; 0x1_8000];
        for byte in palette.iter_mut().take(0x400) {
            *byte = rng.next() as u8;
        }
        Self { texture, palette }
    }
}

impl TextureMemory for Vram {
    fn texture(&self, address: u32) -> u8 {
        self.texture[address as usize % self.texture.len()]
    }

    fn texture_palette(&self, address: u32) -> u8 {
        self.palette[address as usize % self.palette.len()]
    }
}

fn texture_param(format: u32, offset: u32, size: u32, transparent: bool) -> u32 {
    (offset / 8)
        | (0b11 << 16)
        | (size << 20)
        | (size << 23)
        | (format << 26)
        | (u32::from(transparent) << 29)
}

fn cube(gx: &mut Gx, half: i32, texels: i32) {
    let faces: [([i32; 3], [[i32; 3]; 4]); 6] = [
        ([0, 0, 1], [[-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]]),
        (
            [0, 0, -1],
            [[1, -1, -1], [-1, -1, -1], [-1, 1, -1], [1, 1, -1]],
        ),
        ([1, 0, 0], [[1, -1, 1], [1, -1, -1], [1, 1, -1], [1, 1, 1]]),
        (
            [-1, 0, 0],
            [[-1, -1, -1], [-1, -1, 1], [-1, 1, 1], [-1, 1, -1]],
        ),
        ([0, 1, 0], [[-1, 1, 1], [1, 1, 1], [1, 1, -1], [-1, 1, -1]]),
        (
            [0, -1, 0],
            [[-1, -1, -1], [1, -1, -1], [1, -1, 1], [-1, -1, 1]],
        ),
    ];
    let corners = [[0, texels], [texels, texels], [texels, 0], [0, 0]];
    gx.send(BEGIN_VTXS, &[QUADS]);
    for (normal, quad) in faces {
        gx.normal(normal[0] * 511, normal[1] * 511, normal[2] * 511);
        for (corner, [s, t]) in quad.iter().zip(corners) {
            gx.texcoord(s << 4, t << 4);
            gx.vertex(corner[0] * half, corner[1] * half, corner[2] * half);
        }
    }
    gx.send(END_VTXS, &[]);
}

fn cubes(gx: &mut Gx, frame: i32) {
    gx.lights();
    for row in 0..6 {
        for column in 0..8 {
            let index = row * 8 + column;
            gx.send(MTX_PUSH, &[]);
            gx.send(
                MTX_TRANS,
                &[
                    (column - 4) * 0x0C00 + 0x600,
                    (row - 3) * 0x0C00 + 0x600,
                    -9 * ONE,
                ]
                .map(|v| v as u32),
            );
            gx.rotate(frame * 3 + index * 17, frame * 2 + index * 11);
            let textured = if index % 2 == 0 {
                texture_param(7, 0, 2, false)
            } else {
                texture_param(3, 0x1000, 3, true)
            };
            gx.send(TEXIMAGE_PARAM, &[textured]);
            gx.send(PLTT_BASE, &[0]);
            gx.send(
                POLYGON_ATTR,
                &[0b11 | 0b11 << 6 | 31 << 16 | (index as u32 & 0x3F) << 24],
            );
            cube(gx, 0x480, 32);
            gx.send(MTX_POP, &[1]);
        }
    }
}

fn panes(gx: &mut Gx, frame: i32) {
    gx.send(TEXIMAGE_PARAM, &[texture_param(7, 0, 2, false)]);
    gx.send(POLYGON_ATTR, &[0b11 << 6 | 31 << 16]);
    gx.send(MTX_PUSH, &[]);
    gx.send(MTX_TRANS, &[0, 0, (-12 * ONE) as u32]);
    gx.send(MTX_SCALE, &[(8 * ONE) as u32, (6 * ONE) as u32, ONE as u32]);
    gx.send(BEGIN_VTXS, &[QUADS]);
    for (x, y) in [(-1, -1), (1, -1), (1, 1), (-1, 1)] {
        gx.texcoord((x + 1) * 256, (y + 1) * 256);
        gx.vertex(x * ONE / 2, y * ONE / 2, 0);
    }
    gx.send(END_VTXS, &[]);
    gx.send(MTX_POP, &[1]);
    for pane in 0..14 {
        gx.send(MTX_PUSH, &[]);
        gx.send(
            MTX_TRANS,
            &[
                (pane % 5 - 2) * 0x900,
                (pane / 5 - 1) * 0x900,
                (-6 * ONE - pane * 0x200),
            ]
            .map(|v| v as u32),
        );
        gx.rotate(frame * 2 + pane * 25, frame + pane * 40);
        let (format, offset) = if pane % 2 == 0 {
            (1, 0x2000)
        } else {
            (6, 0x3000)
        };
        gx.send(TEXIMAGE_PARAM, &[texture_param(format, offset, 2, false)]);
        gx.send(PLTT_BASE, &[0x10]);
        gx.send(
            POLYGON_ATTR,
            &[0b11 << 6 | (8 + pane as u32 % 20) << 16 | (pane as u32 + 1) << 24 | 1 << 11],
        );
        gx.send(BEGIN_VTXS, &[QUADS]);
        for (x, y) in [(-1, -1), (1, -1), (1, 1), (-1, 1)] {
            gx.texcoord((x + 1) * 256, (y + 1) * 256);
            gx.vertex(x * 0x900, y * 0x700, 0);
        }
        gx.send(END_VTXS, &[]);
        gx.send(MTX_POP, &[1]);
    }
}

fn terrain(gx: &mut Gx, frame: i32) {
    gx.send(POLYGON_ATTR, &[0b11 << 6 | 31 << 16]);
    gx.send(TEXIMAGE_PARAM, &[0]);
    gx.send(MTX_PUSH, &[]);
    gx.send(MTX_TRANS, &[0, (-ONE) as u32, (-6 * ONE) as u32]);
    gx.rotate(frame, 20);
    let (columns, rows) = (40, 24);
    let height = |x: i32, z: i32| (sine(x * 18 + frame * 4) + cosine(z * 23 - frame * 3)) / 6;
    for row in 0..rows {
        gx.send(BEGIN_VTXS, &[STRIPS]);
        for column in 0..=columns {
            for z in [row, row + 1] {
                let shade =
                    (column * 31 / columns) as u32 | ((z * 31 / rows) as u32) << 5 | 0x10 << 10;
                gx.send(COLOR, &[shade]);
                gx.vertex(
                    (column - columns / 2) * 0x0D0,
                    height(column, z),
                    (z - rows / 2) * 0x0D0,
                );
            }
        }
        gx.send(END_VTXS, &[]);
    }
    gx.send(MTX_POP, &[1]);
}

pub(crate) fn three_d(workload: &Workload) -> Result<Outcome, String> {
    let scene: fn(&mut Gx, i32) = match workload.scene.as_deref() {
        Some("cubes") => cubes,
        Some("panes") => panes,
        Some("terrain") => terrain,
        other => return Err(format!("{}: no 3D scene {other:?}", workload.id)),
    };
    let frames = workload.frames as usize;
    let marks = checkpoints(frames);
    let vram = Vram::new(workload.seed);
    let mut geometry = Geometry::default();
    let mut renderer = Renderer::default();
    let mut registers = Registers::default();
    registers.write(DISP3DCNT, TEXTURES_AND_BLENDING, ALL);
    registers.write(CLEAR_COLOR, 0x2C63 | 31 << 16, ALL);
    registers.write(CLEAR_DEPTH, 0x7FFF, ALL);
    let mut shots = Vec::new();
    let mut polygons = 0u64;
    let start = Instant::now();
    crate::run::measured(|| {
        let mut line = Line3d::default();
        let mut gx = Gx(&mut geometry);
        gx.projection();
        for frame in 0..frames {
            scene(&mut gx, frame as i32);
            gx.swap(workload.scene.as_deref() == Some("panes"));
            polygons += gx.0.with_frame(|shown| shown.polygons.len()) as u64;
            renderer.start_frame(registers);
            let mut shot = Fnv::default();
            for y in 0..HEIGHT {
                gx.0.with_frame(|shown| renderer.render_line(y, shown, &vram, &mut line));
                if marks.contains(&(frame + 1)) {
                    for color in line.color {
                        shot.bytes(&color.to_le_bytes());
                    }
                    shot.bytes(&line.alpha);
                }
            }
            if marks.contains(&(frame + 1)) {
                shots.push(shot.finish());
            }
        }
    });
    let wall = start.elapsed();
    let mut facts = std::collections::BTreeMap::new();
    facts.insert("polygons".to_string(), polygons);
    Ok(Outcome {
        digest: Digest {
            frames: shots,
            facts,
            ..Digest::default()
        },
        wall,
    })
}

struct Engine2d {
    bg: Vec<u8>,
    obj: Vec<u8>,
}

impl EngineVram for Engine2d {
    fn bg(&self, address: u32) -> u8 {
        self.bg[address as usize % self.bg.len()]
    }

    fn obj(&self, address: u32) -> u8 {
        self.obj[address as usize % self.obj.len()]
    }

    fn bg_palette(&self, address: u32) -> u8 {
        self.bg[(address as usize ^ 0x5A5A) % self.bg.len()]
    }

    fn obj_palette(&self, address: u32) -> u8 {
        self.obj[(address as usize ^ 0x5A5A) % self.obj.len()]
    }
}

fn oam(rng: &mut Rng, frame: i32) -> Vec<u8> {
    let mut oam = vec![0u8; 0x400];
    for sprite in 0..128 {
        let x = (sprite * 37 + frame * (1 + sprite % 3)).rem_euclid(512) as u16;
        let y = (sprite * 23 + frame).rem_euclid(256) as u16;
        let affine = sprite % 4 == 0;
        let mode = if sprite % 5 == 0 { 1 } else { 0 };
        let depth = u16::from(sprite % 2 == 0);
        let shape = (sprite % 3) as u16;
        let size = (sprite / 3 % 4) as u16;
        let attr0 = y & 0xFF
            | u16::from(affine) << 8
            | u16::from(affine && sprite % 8 == 0) << 9
            | mode << 10
            | depth << 13
            | shape << 14;
        let attr1 = x & 0x1FF
            | if affine {
                ((sprite / 4) as u16 % 32) << 9
            } else {
                0
            }
            | size << 14;
        let attr2 = (rng.next() as u16 & 0x3FF)
            | ((sprite % 4) as u16) << 10
            | ((sprite % 16) as u16) << 12;
        let base = sprite as usize * 8;
        oam[base..base + 2].copy_from_slice(&attr0.to_le_bytes());
        oam[base + 2..base + 4].copy_from_slice(&attr1.to_le_bytes());
        oam[base + 4..base + 6].copy_from_slice(&attr2.to_le_bytes());
    }
    for group in 0..32 {
        let angle = frame * 4 + group as i32 * 11;
        let scale = ONE / 16 + (sine(frame * 2 + group as i32 * 7).abs() >> 4);
        let (s, c) = (
            fixed_mul(sine(angle), scale) >> 4,
            fixed_mul(cosine(angle), scale) >> 4,
        );
        for (slot, value) in [c, -s, s, c].into_iter().enumerate() {
            let at = group * 32 + slot * 8 + 6;
            oam[at..at + 2].copy_from_slice(&(value as i16).to_le_bytes());
        }
    }
    oam
}

pub(crate) fn two_d(workload: &Workload) -> Result<Outcome, String> {
    let frames = workload.frames as usize;
    let marks = checkpoints(frames);
    let mut rng = Rng(workload.seed | 1);
    let mut vram = Engine2d {
        bg: vec![0; 0x8_0000],
        obj: vec![0; 0x4_0000],
    };
    for byte in vram.bg.iter_mut().chain(vram.obj.iter_mut()) {
        *byte = rng.next() as u8;
    }
    for map in 0..4usize {
        let base = 0x1_0000 + map * 0x800;
        for entry in 0..0x400usize {
            let tile = (entry * 7 + map * 131) as u16 & 0x1FF
                | ((entry % 4) as u16) << 10
                | ((entry % 16) as u16) << 12;
            vram.bg[base + entry * 2..base + entry * 2 + 2].copy_from_slice(&tile.to_le_bytes());
        }
    }
    let palette: Vec<u8> = (0..0x400).map(|_| rng.next() as u8).collect();
    let mut engine = Engine::new(EngineKind::A);
    let w = |engine: &mut Engine, offset: usize, value: u16| engine.write(offset, value, 0xFFFF);
    w(&mut engine, 0x00, 0x1F11 | 1 << 13 | 1 << 4);
    w(&mut engine, 0x02, 0x0001);
    w(&mut engine, 0x08, 2 << 8);
    w(&mut engine, 0x0A, 0x0001 | 3 << 8 | 1 << 7);
    w(&mut engine, 0x0C, 0x0002 | 4 << 8);
    w(&mut engine, 0x0E, 0x0003 | 5 << 8 | 1 << 14 | 1 << 13);
    w(&mut engine, 0x48, 0x3F3F);
    w(&mut engine, 0x4A, 0x3F15);
    w(&mut engine, 0x50, 0x0042 | 0x0C << 8);
    w(&mut engine, 0x52, 0x0808);
    let mut shots = Vec::new();
    let mut lines = 0u64;
    let start = Instant::now();
    crate::run::measured(|| {
        let mut out = [0u32; WIDTH];
        for frame in 0..frames as i32 {
            let oam = oam(&mut rng, frame);
            let memory = Memory {
                vram: &vram,
                palette: &palette,
                oam: &oam,
                three_d: None,
            };
            let angle = frame * 2;
            for (offset, value) in [
                (0x30, cosine(angle) >> 4),
                (0x32, -sine(angle) >> 4),
                (0x34, sine(angle) >> 4),
                (0x36, cosine(angle) >> 4),
            ] {
                w(&mut engine, offset, value as u16);
            }
            w(
                &mut engine,
                0x40,
                (((frame % 100) as u16) << 8) | (100 + (frame % 100) as u16),
            );
            w(&mut engine, 0x44, 20 << 8 | 170);
            let mut shot = Fnv::default();
            for y in 0..LINES {
                engine.start_line(y);
                if usize::from(y) < HEIGHT {
                    w(
                        &mut engine,
                        0x10,
                        (frame as u16)
                            .wrapping_add((sine(i32::from(y) * 6 + frame * 5) >> 9) as u16),
                    );
                    w(&mut engine, 0x16, (frame as u16).wrapping_mul(2));
                    engine.render_line(usize::from(y), &memory, &mut out);
                    engine.master_brightness(&mut out);
                    lines += 1;
                    if marks.contains(&(frame as usize + 1)) {
                        for color in out {
                            shot.bytes(&color.to_le_bytes());
                        }
                    }
                }
            }
            if marks.contains(&(frame as usize + 1)) {
                shots.push(shot.finish());
            }
        }
    });
    let wall = start.elapsed();
    let mut facts = std::collections::BTreeMap::new();
    facts.insert("lines".to_string(), lines);
    Ok(Outcome {
        digest: Digest {
            frames: shots,
            facts,
            ..Digest::default()
        },
        wall,
    })
}

pub(crate) fn system(workload: &Workload, rom: &[u8]) -> Result<Outcome, String> {
    let frames = workload.frames as usize;
    let marks = checkpoints(frames);
    let inputs = crate::input::script(workload.seed, frames);
    let touch = workload.script.as_deref() == Some("touch");
    let mut nds = Nds::new(rom.into(), NdsFirmware::default()).map_err(|e| e.to_string())?;
    nds.set_clock(START_TIME);
    let mut rng = Rng(workload.seed | 1);
    let mut shots = Vec::new();
    let mut touches = 0u64;
    let start = Instant::now();
    crate::run::measured(|| {
        for (frame, input) in inputs.iter().enumerate() {
            nds.set_keys(input.buttons);
            if touch {
                let pen = (frame % 40 < 30).then(|| Touch {
                    x: (rng.next() % 256) as u8,
                    y: (rng.next() % HEIGHT as u32) as u8,
                });
                touches += u64::from(pen.is_some());
                nds.set_touch(pen);
            }
            nds.run_frame();
            if marks.contains(&(frame + 1)) {
                shots.push(crate::digest::hash(nds.frame()));
            }
        }
    });
    let wall = start.elapsed();
    if let Some(fault) = nds.faults().into_iter().flatten().next() {
        return Err(format!(
            "{}: the {:?} stopped at {:08X}",
            workload.id, fault.cpu, fault.pc
        ));
    }
    let mut state = Fnv::default();
    state.u64(nds.cycles());
    state.u64(nds.frames());
    state.bytes(nds.buses().palette());
    state.bytes(nds.buses().oam());
    for bank in Bank::ALL {
        state.bytes(nds.buses().vram().bank(bank));
    }
    let mut facts = std::collections::BTreeMap::new();
    facts.insert("cycles".to_string(), nds.cycles());
    if touch {
        facts.insert("touches".to_string(), touches);
    }
    Ok(Outcome {
        digest: Digest {
            frames: shots,
            state: state.finish(),
            facts,
            ..Digest::default()
        },
        wall,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_integer_sine_hits_its_key_points() {
        assert_eq!(sine(0), 0);
        assert_eq!(sine(90), ONE);
        assert_eq!(sine(270), -ONE);
        assert_eq!(cosine(0), ONE);
    }
}
