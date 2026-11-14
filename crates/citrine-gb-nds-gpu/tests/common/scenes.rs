use super::*;

pub const TEXTURES: u32 = 1 << 0;
pub const ALPHA_TEST: u32 = 1 << 2;
pub const BLENDING: u32 = 1 << 3;
pub const ANTIALIASING: u32 = 1 << 4;
pub const EDGE_MARKING: u32 = 1 << 5;
pub const FOG_ALPHA_ONLY: u32 = 1 << 6;
pub const FOG_ENABLE: u32 = 1 << 7;
pub const FAR_DEPTH: u32 = 0x7FFF;

const SOLID: u32 = 31 << 16;
const SIZE_8: u32 = 0;
const SIZE_32: u32 = 2;

const A3I5: u32 = 1;
const FOUR_COLORS: u32 = 2;
const SIXTEEN_COLORS: u32 = 3;
const PALETTE_256: u32 = 4;
const COMPRESSED: u32 = 5;
const A5I3: u32 = 6;
const DIRECT: u32 = 7;

pub fn all() -> Vec<Scene> {
    vec![
        shapes(),
        w_buffer(),
        textures(),
        translucency(),
        shadows(),
        wireframe_and_equal_depth(),
        rear_plane(),
    ]
}

fn quad(gx: &mut Gx, corners: [[f32; 3]; 4], colors: [u16; 4]) {
    gx.begin(QUADS);
    for (corner, color) in corners.into_iter().zip(colors) {
        gx.color(color);
        gx.vertex(corner[0], corner[1], corner[2]);
    }
    gx.end();
}

fn textured_quad(gx: &mut Gx, corners: [[f32; 3]; 4], size: f32, color: u16) {
    let texcoords = [[0.0, 0.0], [0.0, size], [size, size], [size, 0.0]];
    gx.begin(QUADS);
    gx.color(color);
    for (corner, [s, t]) in corners.into_iter().zip(texcoords) {
        gx.texcoord(s, t);
        gx.vertex(corner[0], corner[1], corner[2]);
    }
    gx.end();
}

fn square(x: f32, y: f32, half: f32) -> [[f32; 3]; 4] {
    [
        [x - half, y + half, 0.0],
        [x - half, y - half, 0.0],
        [x + half, y - half, 0.0],
        [x + half, y + half, 0.0],
    ]
}

fn shapes_geometry(gx: &mut Gx, extra: u32) {
    gx.perspective(0.5, 20.0);
    gx.push();
    gx.translate(0.0, -1.0, -4.0);
    gx.attributes(BOTH | SOLID | id(1) | extra);
    quad(
        gx,
        [
            [-3.0, 0.0, -3.0],
            [-3.0, 0.0, 3.0],
            [3.0, 0.0, 3.0],
            [3.0, 0.0, -3.0],
        ],
        [rgb(31, 4, 4), rgb(4, 31, 4), rgb(4, 4, 31), rgb(31, 31, 4)],
    );
    gx.pop();
    gx.push();
    gx.translate(-0.6, 0.0, -3.5);
    gx.rotate_y(35.0);
    gx.attributes(BOTH | SOLID | id(2) | extra);
    quad(
        gx,
        square(0.0, 0.0, 0.8),
        [
            rgb(31, 0, 31),
            rgb(0, 31, 31),
            rgb(31, 31, 31),
            rgb(10, 10, 10),
        ],
    );
    gx.pop();
    gx.push();
    gx.translate(-0.4, 0.1, -3.6);
    gx.rotate_y(-40.0);
    gx.attributes(FRONT | SOLID | id(3) | extra);
    quad(
        gx,
        square(0.0, 0.0, 0.7),
        [
            rgb(31, 20, 0),
            rgb(31, 20, 0),
            rgb(0, 20, 31),
            rgb(0, 20, 31),
        ],
    );
    gx.pop();
    gx.push();
    gx.translate(1.3, 0.4, -3.0);
    gx.rotate_x(60.0);
    gx.rotate_y(20.0);
    gx.attributes(BOTH | SOLID | id(4) | extra);
    gx.begin(TRIANGLES);
    for (color, [x, y]) in [
        (rgb(31, 0, 0), [0.0, 0.9]),
        (rgb(0, 31, 0), [0.9, -0.6]),
        (rgb(0, 0, 31), [-0.9, -0.6]),
    ] {
        gx.color(color);
        gx.vertex(x, y, 0.0);
    }
    gx.end();
    gx.pop();
    gx.push();
    gx.translate(2.2, -0.2, -2.5);
    gx.rotate_y(70.0);
    gx.attributes(FRONT | SOLID | id(5) | extra);
    quad(
        gx,
        square(0.0, 0.0, 0.9),
        [
            rgb(20, 20, 31),
            rgb(5, 5, 20),
            rgb(20, 20, 31),
            rgb(5, 5, 20),
        ],
    );
    gx.pop();
}

pub fn shapes() -> Scene {
    let mut gx = Gx::default();
    shapes_geometry(&mut gx, 0);
    Scene {
        name: "shapes",
        frame: gx.finish(false, false),
        registers: registers(0, u32::from(rgb(2, 2, 6)) | SOLID, FAR_DEPTH, 0),
        memory: Memory::default(),
    }
}

pub fn w_buffer() -> Scene {
    let mut gx = Gx::default();
    shapes_geometry(&mut gx, 0);
    Scene {
        name: "w-buffer",
        frame: gx.finish(true, false),
        registers: registers(0, u32::from(rgb(2, 2, 6)) | SOLID, FAR_DEPTH, 0),
        memory: Memory::default(),
    }
}

pub fn edge_marked() -> Scene {
    let mut gx = Gx::default();
    shapes_geometry(&mut gx, 0);
    let mut registers = registers(EDGE_MARKING, u32::from(rgb(2, 2, 6)) | SOLID, FAR_DEPTH, 0);
    let colors = [
        rgb(31, 31, 31),
        rgb(31, 0, 0),
        rgb(0, 31, 0),
        rgb(0, 0, 31),
        rgb(31, 31, 0),
        rgb(0, 31, 31),
        rgb(31, 0, 31),
        rgb(16, 16, 16),
    ];
    for (word, pair) in colors.chunks(2).enumerate() {
        let value = u32::from(pair[0]) | u32::from(pair[1]) << 16;
        registers.write(EDGE_COLOR + word * 4, value, !0);
    }
    Scene {
        name: "edge marking",
        frame: gx.finish(false, false),
        registers,
        memory: Memory::default(),
    }
}

fn with_fog(mut registers: Registers) -> Registers {
    registers.write(FOG_COLOR, u32::from(rgb(24, 26, 31)) | 12 << 16, !0);
    registers.write(FOG_OFFSET, 0x2000, !0);
    for word in 0..8 {
        let value = u32::from_le_bytes([0, 1, 2, 3].map(|byte| ((word * 4 + byte) * 4) as u8));
        registers.write(FOG_TABLE + word as usize * 4, value, !0);
    }
    registers
}

pub fn fogged() -> Scene {
    let mut gx = Gx::default();
    shapes_geometry(&mut gx, FOG);
    Scene {
        name: "fog",
        frame: gx.finish(false, false),
        registers: with_fog(registers(
            FOG_ENABLE | 1 << 8,
            u32::from(rgb(2, 2, 6)) | SOLID | FOG,
            FAR_DEPTH,
            0,
        )),
        memory: Memory::default(),
    }
}

pub fn fogged_alpha() -> Scene {
    let mut scene = translucency();
    scene.name = "fog on alpha";
    scene.registers = with_fog(registers(
        TEXTURES | BLENDING | ALPHA_TEST | FOG_ENABLE | FOG_ALPHA_ONLY,
        u32::from(rgb(4, 4, 8)) | SOLID | FOG,
        FAR_DEPTH,
        6,
    ));
    scene
}

pub fn rear_plane() -> Scene {
    let mut gx = Gx::default();
    shapes_geometry(&mut gx, 0);
    Scene {
        name: "rear plane",
        frame: gx.finish(false, false),
        registers: registers(
            0,
            u32::from(rgb(6, 2, 2)) | 20 << 16 | id(9) | FOG,
            0x7E00,
            0,
        ),
        memory: Memory::default(),
    }
}

pub fn texture_memory() -> Memory {
    let mut memory = Memory::default();
    for index in 0..256u16 {
        let color = rgb(index % 32, (index * 7 + 5) % 32, 31 - index % 32);
        memory.set_color(u32::from(index) * 2, color);
    }
    for t in 0..32u32 {
        for s in 0..32u32 {
            let texel = (t * 32 + s) as usize;
            memory.texture[texel] = ((s / 4) << 5 | t) as u8;
            let four = ((s / 4 + t / 4) % 4) << ((s & 3) * 2);
            memory.texture[0x400 + texel / 4] |= four as u8;
            let sixteen = ((s / 2 + t / 4) % 16) << ((s & 1) * 4);
            memory.texture[0x800 + texel / 2] |= sixteen as u8;
            memory.texture[0x1000 + texel] = ((s * 8 + t) & 0xFF) as u8;
            memory.texture[0x3000 + texel] = (s << 3 | (t / 4)) as u8;
            let (dx, dy) = (s as i32 - 16, t as i32 - 16);
            let hole = dx * dx + dy * dy < 36;
            let direct = rgb(s as u16, t as u16, 31 - s as u16) | if hole { 0 } else { 0x8000 };
            memory.set_texture_u16(0x4000 + texel as u32 * 2, direct);
        }
    }
    for block_t in 0..8u32 {
        for block_s in 0..8u32 {
            let block = block_t * 8 + block_s;
            let address = 0x8000 + block_t * 32 + block_s * 4;
            for row in 0..4 {
                let codes = (block * 37 + row * 11) as u8;
                memory.texture[(address + row) as usize] = codes;
            }
            let index_address = 0x20000 + ((address & 0x1FFFF) >> 1);
            let info = (block * 3 % 40) as u16 | ((block % 4) as u16) << 14;
            memory.set_texture_u16(index_address, info);
        }
    }
    memory
}

pub fn textures() -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    let formats = [
        (A3I5, 0x0000, MODULATE, 0),
        (FOUR_COLORS, 0x0400, MODULATE, 0),
        (SIXTEEN_COLORS, 0x0800, DECAL, 0),
        (PALETTE_256, 0x1000, MODULATE, 1),
        (COMPRESSED, 0x8000, MODULATE, 0),
        (A5I3, 0x3000, DECAL, 0),
        (DIRECT, 0x4000, DECAL, 0),
        (DIRECT, 0x4000, MODULATE, 0),
    ];
    for (index, (format, offset, mode, palette)) in formats.into_iter().enumerate() {
        let (column, row) = ((index % 4) as f32, (index / 4) as f32);
        gx.push();
        gx.translate(-2.1 + column * 1.4, 0.75 - row * 1.5, -4.0);
        gx.rotate_x(-35.0 + row * 25.0);
        gx.rotate_y(20.0 - column * 12.0);
        gx.attributes(FRONT | mode | SOLID | id(10 + index as u32));
        let flip = index == 1 || index == 6;
        gx.texture(texture_param(format, offset, SIZE_32, true, flip), palette);
        let size = if flip { 64.0 } else { 32.0 };
        textured_quad(&mut gx, square(0.0, 0.0, 0.6), size, rgb(31, 28, 20));
        gx.pop();
    }
    gx.push();
    gx.translate(0.0, 0.0, -6.0);
    gx.attributes(FRONT | SOLID | id(30));
    gx.texture(texture_param(PALETTE_256, 0x1000, SIZE_32, false, false), 0);
    gx.begin(QUADS);
    gx.color(rgb(31, 31, 31));
    for ([s, t], [x, y]) in [
        ([-16.0, -16.0], [-4.0, 3.0]),
        ([-16.0, 48.0], [-4.0, -3.0]),
        ([48.0, 48.0], [4.0, -3.0]),
        ([48.0, -16.0], [4.0, 3.0]),
    ] {
        gx.texcoord(s, t);
        gx.vertex(x, y, 0.0);
    }
    gx.end();
    gx.pop();
    Scene {
        name: "textures",
        frame: gx.finish(false, false),
        registers: registers(
            TEXTURES | BLENDING,
            u32::from(rgb(0, 0, 0)) | SOLID,
            FAR_DEPTH,
            0,
        ),
        memory: texture_memory(),
    }
}

pub fn translucency() -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.push();
    gx.translate(0.0, 0.0, -5.0);
    gx.attributes(FRONT | SOLID | id(1));
    quad(
        &mut gx,
        square(0.0, 0.0, 2.5),
        [rgb(31, 31, 31), rgb(0, 0, 0), rgb(31, 31, 31), rgb(0, 0, 0)],
    );
    gx.pop();
    let panes = [
        (-0.8, 0.5, -4.0, alpha(16) | id(2), rgb(31, 0, 0)),
        (-0.2, 0.2, -3.8, alpha(16) | id(2), rgb(0, 31, 0)),
        (
            0.4,
            -0.1,
            -3.6,
            alpha(10) | id(3) | TRANSLUCENT_DEPTH,
            rgb(0, 0, 31),
        ),
        (0.6, 0.6, -3.4, alpha(24) | id(4) | FOG, rgb(31, 31, 0)),
        (-0.6, -0.6, -3.2, alpha(4) | id(5), rgb(31, 0, 31)),
        (0.2, -0.5, -4.5, alpha(20) | id(6), rgb(0, 31, 31)),
    ];
    for (x, y, z, attributes, color) in panes {
        gx.push();
        gx.translate(x, y, z);
        gx.rotate_y(15.0);
        gx.attributes(FRONT | attributes);
        quad(&mut gx, square(0.0, 0.0, 0.7), [color; 4]);
        gx.pop();
    }
    gx.push();
    gx.translate(-1.5, -1.0, -3.5);
    gx.attributes(FRONT | MODULATE | SOLID | id(7));
    gx.texture(texture_param(A3I5, 0x0000, SIZE_32, true, false), 0);
    textured_quad(&mut gx, square(0.0, 0.0, 0.6), 32.0, rgb(31, 31, 31));
    gx.pop();
    Scene {
        name: "translucency",
        frame: gx.finish(false, false),
        registers: registers(
            TEXTURES | BLENDING | ALPHA_TEST,
            u32::from(rgb(4, 4, 8)) | SOLID,
            FAR_DEPTH,
            6,
        ),
        memory: texture_memory(),
    }
}

fn cube(gx: &mut Gx, half: [f32; 3], color: u16) {
    let [x, y, z] = half;
    let faces = [
        [[-x, y, z], [-x, -y, z], [x, -y, z], [x, y, z]],
        [[x, y, -z], [x, -y, -z], [-x, -y, -z], [-x, y, -z]],
        [[x, y, z], [x, -y, z], [x, -y, -z], [x, y, -z]],
        [[-x, y, -z], [-x, -y, -z], [-x, -y, z], [-x, y, z]],
        [[-x, y, -z], [-x, y, z], [x, y, z], [x, y, -z]],
        [[-x, -y, z], [-x, -y, -z], [x, -y, -z], [x, -y, z]],
    ];
    for face in faces {
        quad(gx, face, [color; 4]);
    }
}

pub fn shadows() -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.push();
    gx.translate(0.0, -1.0, -4.0);
    gx.attributes(FRONT | SOLID | id(1));
    quad(
        &mut gx,
        [
            [-3.0, 0.0, -3.0],
            [-3.0, 0.0, 3.0],
            [3.0, 0.0, 3.0],
            [3.0, 0.0, -3.0],
        ],
        [rgb(28, 28, 20); 4],
    );
    gx.pop();
    gx.push();
    gx.translate(0.3, -0.2, -4.0);
    gx.rotate_y(25.0);
    gx.attributes(FRONT | SOLID | id(2));
    cube(&mut gx, [0.4, 0.4, 0.4], rgb(8, 20, 31));
    gx.pop();
    for (x, z) in [(-0.4, -3.7), (0.9, -4.4)] {
        gx.push();
        gx.translate(x, -0.8, z);
        gx.rotate_y(10.0);
        gx.attributes(BACK | SHADOW | alpha(18) | id(0));
        cube(&mut gx, [0.5, 0.6, 0.5], rgb(0, 0, 0));
        gx.attributes(FRONT | SHADOW | alpha(18) | id(6));
        cube(&mut gx, [0.5, 0.6, 0.5], rgb(0, 0, 4));
        gx.pop();
    }
    Scene {
        name: "shadows",
        frame: gx.finish(false, true),
        registers: registers(BLENDING, u32::from(rgb(6, 8, 10)) | SOLID, FAR_DEPTH, 0),
        memory: Memory::default(),
    }
}

pub fn wireframe_and_equal_depth() -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    let floor = [
        [-2.5, 0.0, -2.5],
        [-2.5, 0.0, 2.5],
        [2.5, 0.0, 2.5],
        [2.5, 0.0, -2.5],
    ];
    gx.push();
    gx.translate(0.0, -1.0, -4.0);
    gx.attributes(FRONT | SOLID | id(1));
    quad(&mut gx, floor, [rgb(10, 20, 10); 4]);
    gx.attributes(FRONT | DECAL | SOLID | DEPTH_EQUAL | id(2));
    gx.texture(texture_param(PALETTE_256, 0x1000, SIZE_32, true, false), 1);
    textured_quad(&mut gx, floor, 64.0, rgb(31, 31, 31));
    gx.pop();
    gx.push();
    gx.translate(0.5, 0.3, -3.5);
    gx.rotate_y(30.0);
    gx.attributes(BOTH | id(3));
    quad(
        &mut gx,
        square(0.0, 0.0, 0.8),
        [rgb(31, 0, 0), rgb(0, 31, 0), rgb(0, 0, 31), rgb(31, 31, 31)],
    );
    gx.pop();
    gx.push();
    gx.translate(-1.2, 0.4, -3.8);
    gx.rotate_x(30.0);
    gx.attributes(FRONT | id(4));
    gx.begin(TRIANGLES);
    for [x, y] in [[0.0, 0.7], [0.7, -0.5], [-0.7, -0.5]] {
        gx.color(rgb(31, 31, 0));
        gx.vertex(x, y, 0.0);
    }
    gx.end();
    gx.pop();
    Scene {
        name: "wireframe and equal depth",
        frame: gx.finish(false, false),
        registers: registers(TEXTURES, u32::from(rgb(0, 0, 4)) | SOLID, FAR_DEPTH, 0),
        memory: texture_memory(),
    }
}

pub const MESH_REAR: u16 = 0x7C1F;

fn mesh_color(index: usize) -> u16 {
    let level = (index * 7 % 24) as u16;
    rgb(0, 6 + level, 31 - level)
}

fn mesh_scene(name: &'static str, gx: Gx) -> Scene {
    Scene {
        name,
        frame: gx.finish(false, false),
        registers: registers(0, u32::from(MESH_REAR) | SOLID, FAR_DEPTH, 0),
        memory: Memory::default(),
    }
}

fn textured_mesh_scene(name: &'static str, gx: Gx) -> Scene {
    Scene {
        name,
        frame: gx.finish(false, false),
        registers: registers(TEXTURES, u32::from(MESH_REAR) | SOLID, FAR_DEPTH, 0),
        memory: texture_memory(),
    }
}

fn mesh_vertex(gx: &mut Gx, textured: bool, x: f32, y: f32) {
    if textured {
        gx.texcoord(x * 24.0, y * 24.0);
    }
    gx.vertex(x, y, 0.0);
}

fn mesh_texture(gx: &mut Gx, textured: bool) {
    if textured {
        gx.texture(texture_param(PALETTE_256, 0x1000, SIZE_32, true, false), 0);
    }
}

pub fn fan() -> Scene {
    mesh_scene("fan", fan_mesh(false))
}

pub fn textured_fan() -> Scene {
    textured_mesh_scene("textured fan", fan_mesh(true))
}

fn fan_mesh(textured: bool) -> Gx {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.translate(0.1, 0.05, -3.3);
    gx.rotate_x(55.0);
    gx.rotate_y(17.0);
    gx.attributes(BOTH | SOLID | id(1));
    mesh_texture(&mut gx, textured);
    gx.begin(TRIANGLES);
    let spokes = 17;
    let point = |index: usize| {
        let angle = index as f32 / spokes as f32 * std::f32::consts::TAU;
        [1.4 * angle.cos(), 1.4 * angle.sin()]
    };
    for index in 0..spokes {
        for (vertex, [x, y]) in [[0.0, 0.0], point(index), point(index + 1)]
            .into_iter()
            .enumerate()
        {
            gx.color(mesh_color(index * 3 + vertex));
            mesh_vertex(&mut gx, textured, x, y);
        }
    }
    gx.end();
    gx
}

pub fn strip() -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.translate(-0.05, 0.1, -3.0);
    gx.rotate_x(-35.0);
    gx.rotate_y(28.0);
    gx.attributes(BOTH | SOLID | id(1));
    gx.begin(TRIANGLE_STRIP);
    for index in 0..24 {
        let x = -1.6 + index as f32 / 2.0 * 0.29;
        let y = if index % 2 == 0 { 0.7 } else { -0.7 };
        gx.color(mesh_color(index));
        gx.vertex(x, y, 0.0);
    }
    gx.end();
    mesh_scene("strip", gx)
}

pub fn t_junctions() -> Scene {
    mesh_scene("t-junctions", t_junction_mesh(false))
}

pub fn textured_t_junctions() -> Scene {
    textured_mesh_scene("textured t-junctions", t_junction_mesh(true))
}

fn t_junction_mesh(textured: bool) -> Gx {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.translate(0.07, -0.03, -3.1);
    gx.rotate_x(48.0);
    gx.rotate_y(-23.0);
    gx.attributes(BOTH | SOLID | id(1));
    mesh_texture(&mut gx, textured);
    let mut cell = 0;
    let mut quad_at = |gx: &mut Gx, left: f32, top: f32, right: f32, bottom: f32| {
        gx.begin(QUADS);
        for [x, y] in [[left, top], [left, bottom], [right, bottom], [right, top]] {
            gx.color(mesh_color(cell));
            mesh_vertex(gx, textured, x, y);
        }
        gx.end();
        cell += 1;
    };
    quad_at(&mut gx, -1.5, 1.2, -0.3, -1.2);
    let rows = [1.2, 0.71, 0.33, -0.13, -0.58, -1.2];
    for pair in rows.windows(2) {
        quad_at(&mut gx, -0.3, pair[0], 0.37, pair[1]);
    }
    let rows = [1.2, 0.95, 0.52, 0.11, -0.27, -0.81, -1.2];
    for pair in rows.windows(2) {
        quad_at(&mut gx, 0.37, pair[0], 1.5, pair[1]);
    }
    gx
}

pub fn clipped_floor() -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.translate(0.13, -0.8, 0.0);
    gx.rotate_y(9.0);
    gx.attributes(BOTH | SOLID | id(1));
    let cells = 8;
    for row in 0..cells {
        for column in 0..cells {
            let x = -4.0 + column as f32;
            let z = 1.5 - row as f32;
            gx.begin(QUADS);
            for [dx, dz] in [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]] {
                gx.color(mesh_color(row * cells + column));
                gx.vertex(x + dx, 0.0, z - 1.0 + dz);
            }
            gx.end();
        }
    }
    mesh_scene("clipped floor", gx)
}

pub fn meshes() -> Vec<Scene> {
    vec![fan(), strip(), t_junctions(), clipped_floor()]
}

pub const FLOOR_COLOR: u16 = 0x0280;
pub const SIGN_COLOR: u16 = 0x001F;
pub const SIGN_EDGE: u16 = 0x0C63;

pub fn sign_over_floor() -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.push();
    gx.translate(0.0, 0.0, -6.0);
    gx.attributes(BOTH | SOLID | id(1));
    quad(&mut gx, square(0.0, 0.0, 8.0), [FLOOR_COLOR; 4]);
    gx.pop();
    gx.push();
    gx.translate(0.0, 2.3, -4.0);
    gx.attributes(BOTH | SOLID | id(8));
    quad(&mut gx, square(0.0, 0.0, 0.8), [SIGN_COLOR; 4]);
    gx.pop();
    let mut registers = registers(
        EDGE_MARKING | ANTIALIASING,
        u32::from(rgb(2, 2, 6)) | 63 << 24,
        FAR_DEPTH,
        0,
    );
    registers.write(
        EDGE_COLOR,
        u32::from(rgb(31, 31, 31)) | u32::from(SIGN_EDGE) << 16,
        !0,
    );
    Scene {
        name: "sign over floor",
        frame: gx.finish(false, false),
        registers,
        memory: Memory::default(),
    }
}

pub const HALO_COLOR: u16 = 0x23FF;

pub fn halo(clear_color: u32) -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.push();
    gx.translate(-0.7, 0.0, -3.3);
    gx.rotate_x(40.0);
    gx.rotate_y(23.0);
    gx.attributes(BOTH | SOLID | id(1));
    gx.begin(TRIANGLES);
    gx.color(HALO_COLOR);
    let spokes = 13;
    let point = |index: usize| {
        let angle = index as f32 / spokes as f32 * std::f32::consts::TAU;
        [0.9 * angle.cos(), 0.9 * angle.sin()]
    };
    for index in 0..spokes {
        for [x, y] in [[0.0, 0.0], point(index), point(index + 1)] {
            gx.vertex(x, y, 0.0);
        }
    }
    gx.end();
    gx.pop();
    gx.push();
    gx.translate(1.0, 0.1, -3.0);
    gx.rotate_y(30.0);
    gx.attributes(BOTH | SOLID | id(2));
    gx.begin(TRIANGLES);
    gx.color(HALO_COLOR);
    for [x, y] in [[0.0, 0.9], [0.2, -0.8], [-0.5, -0.7]] {
        gx.vertex(x, y, 0.0);
    }
    gx.end();
    gx.pop();
    Scene {
        name: "halo",
        frame: gx.finish(false, false),
        registers: registers(0, clear_color, FAR_DEPTH, 0),
        memory: Memory::default(),
    }
}

const STACK_BACK: u16 = 0x03E0;
const STACK_FRONT: u16 = 0x7C00;

pub fn stacked() -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.attributes(BOTH | SOLID | id(1));
    for (color, half, depth, turn) in [(STACK_BACK, 2.0, -4.0, 0.0), (STACK_FRONT, 0.5, -2.5, 25.0)]
    {
        gx.push();
        gx.translate(0.0, 0.0, depth);
        gx.rotate_y(turn);
        gx.begin(QUADS);
        gx.color(color);
        for [x, y] in [[-half, half], [-half, -half], [half, -half], [half, half]] {
            gx.vertex(x, y, 0.0);
        }
        gx.end();
        gx.pop();
    }
    Scene {
        name: "stacked",
        frame: gx.finish(false, false),
        registers: registers(0, u32::from(MESH_REAR) | SOLID, FAR_DEPTH, 0),
        memory: Memory::default(),
    }
}

fn wide_textured_quad(gx: &mut Gx, half: [f32; 2], s: [f32; 2], t: [f32; 2]) {
    let [x, y] = half;
    gx.begin(QUADS);
    gx.color(rgb(31, 31, 31));
    for ([cx, cy], [cs, ct]) in [
        ([-x, y], [s[0], t[0]]),
        ([-x, -y], [s[0], t[1]]),
        ([x, -y], [s[1], t[1]]),
        ([x, y], [s[1], t[0]]),
    ] {
        gx.texcoord(cs, ct);
        gx.vertex(cx, cy, 0.0);
    }
    gx.end();
}

pub fn cut_out() -> Scene {
    let mut memory = Memory::default();
    for t in 0..8u32 {
        for s in 0..8u32 {
            let texel = if s < 4 { rgb(31, 31, 31) | 0x8000 } else { 0 };
            memory.set_texture_u16((t * 8 + s) * 2, texel);
        }
    }
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.translate(0.0, 0.0, -4.0);
    gx.attributes(FRONT | MODULATE | SOLID | id(1));
    gx.texture(texture_param(DIRECT, 0, SIZE_8, false, false), 0);
    wide_textured_quad(&mut gx, [1.5, 1.5], [0.0, 8.0], [0.0, 8.0]);
    Scene {
        name: "cut-out",
        frame: gx.finish(false, false),
        registers: registers(
            TEXTURES | BLENDING,
            u32::from(rgb(31, 31, 31)),
            FAR_DEPTH,
            0,
        ),
        memory,
    }
}

pub fn wrap_edges(repeat: bool, flip: bool) -> Scene {
    let mut memory = Memory::default();
    for t in 0..8u32 {
        for s in 0..8u32 {
            let texel = match s {
                0 => rgb(31, 0, 0),
                7 => rgb(0, 0, 31),
                _ => rgb(0, 31, 0),
            };
            memory.set_texture_u16((t * 8 + s) * 2, texel | 0x8000);
        }
    }
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.translate(0.0, 0.0, -4.0);
    gx.attributes(FRONT | MODULATE | SOLID | id(1));
    gx.texture(texture_param(DIRECT, 0, SIZE_8, repeat, flip), 0);
    wide_textured_quad(&mut gx, [2.0, 0.5], [-8.0, 24.0], [0.0, 8.0]);
    Scene {
        name: "wrap edges",
        frame: gx.finish(false, false),
        registers: registers(TEXTURES, 0, FAR_DEPTH, 0),
        memory,
    }
}

pub const BAND_COLORS: [u16; 2] = [0x001F, 0x03E0];

pub fn bands(tilt: f32) -> Scene {
    let mut gx = Gx::default();
    gx.perspective(0.5, 20.0);
    gx.translate(0.0, 0.03, -3.0);
    gx.rotate_x(tilt);
    gx.attributes(BOTH | SOLID | id(1));
    let count = 13;
    for band in 0..count {
        let top = 1.0 - band as f32 * 2.0 / count as f32;
        let bottom = top - 2.0 / count as f32;
        gx.begin(QUADS);
        for [x, y] in [[-1.3, top], [-1.3, bottom], [1.3, bottom], [1.3, top]] {
            gx.color(BAND_COLORS[band % 2]);
            gx.vertex(x, y, 0.0);
        }
        gx.end();
    }
    mesh_scene("bands", gx)
}
