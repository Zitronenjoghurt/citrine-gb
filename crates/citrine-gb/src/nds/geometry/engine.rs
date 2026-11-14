use super::command::*;
use super::frame::Frame;
use super::light::{self, Lighting};
use super::matrix::{self, Matrices, Matrix};
use super::polygon::{Point, Polygons};
use super::query;
use super::texcoord::{self, Texcoords};
use super::*;

pub const VBLANK: u8 = 0xF0;
pub const ACKNOWLEDGE_STACK: u8 = 0xF1;
pub const ACKNOWLEDGE_OVERFLOW: u8 = 0xF2;
pub const SET_DOT_DEPTH: u8 = 0xF3;

#[derive(Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Engine {
    pub(super) matrices: Matrices,
    polygons: Polygons,
    vertex: [i16; 3],
    color: [u8; 3],
    texcoords: Texcoords,
    lighting: Lighting,
    position_result: [i32; 4],
    vector_result: [i16; 3],
    box_inside: bool,
}

impl Clone for Engine {
    fn clone(&self) -> Self {
        Self {
            matrices: self.matrices.clone(),
            polygons: self.polygons.clone(),
            vertex: self.vertex,
            color: self.color,
            texcoords: self.texcoords,
            lighting: self.lighting.clone(),
            position_result: self.position_result,
            vector_result: self.vector_result,
            box_inside: self.box_inside,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            matrices,
            polygons,
            vertex,
            color,
            texcoords,
            lighting,
            position_result,
            vector_result,
            box_inside,
        } = self;
        matrices.clone_from(&source.matrices);
        polygons.clone_from(&source.polygons);
        vertex.clone_from(&source.vertex);
        color.clone_from(&source.color);
        texcoords.clone_from(&source.texcoords);
        lighting.clone_from(&source.lighting);
        position_result.clone_from(&source.position_result);
        vector_result.clone_from(&source.vector_result);
        box_inside.clone_from(&source.box_inside);
    }
}

impl Engine {
    pub fn read(&self, offset: usize) -> u32 {
        match offset {
            RAM_COUNT => {
                let frame = self.polygons.building();
                frame.polygons.len() as u32 | (frame.vertices.len() as u32) << VERTEX_COUNT_SHIFT
            }
            POS_RESULT..POS_RESULT_END => self.position_result[(offset - POS_RESULT) >> 2] as u32,
            VEC_RESULT => {
                let [x, y, _] = self.vector_result.map(|axis| u32::from(axis as u16));
                x | y << 16
            }
            VEC_RESULT_Z => u32::from(self.vector_result[2] as u16),
            CLIPMTX_RESULT..CLIPMTX_RESULT_END => {
                self.matrices.clip().0[(offset - CLIPMTX_RESULT) >> 2] as u32
            }
            VECMTX_RESULT..VECMTX_RESULT_END => {
                let index = (offset - VECMTX_RESULT) >> 2;
                self.matrices.vector().0[index / 3 * 4 + index % 3] as u32
            }
            _ => 0,
        }
    }

    pub fn box_inside(&self) -> bool {
        self.box_inside
    }

    pub fn frame(&self) -> &Frame {
        self.polygons.frame()
    }

    pub fn overflow(&self) -> bool {
        self.polygons.overflow()
    }

    pub fn run(&mut self, words: &[u32]) {
        let mut rest = words;
        while let Some((&header, tail)) = rest.split_first() {
            let (params, next) = tail.split_at((header >> 8) as usize);
            self.execute(header as u8, params);
            rest = next;
        }
    }

    pub fn execute(&mut self, command: u8, params: &[u32]) {
        let param = params.first().copied().unwrap_or(0);
        match command {
            MTX_MODE => self.matrices.set_mode(param),
            MTX_PUSH => self.matrices.push(),
            MTX_POP => self.matrices.pop(param),
            MTX_STORE => self.matrices.store(param),
            MTX_RESTORE => self.matrices.restore(param),
            MTX_IDENTITY => self.matrices.identity(),
            MTX_LOAD_4X4 => self.matrices.load(Matrix::from_4x4(params)),
            MTX_LOAD_4X3 => self.matrices.load(Matrix::from_4x3(params)),
            MTX_MULT_4X4 => self.matrices.multiply(Matrix::from_4x4(params)),
            MTX_MULT_4X3 => self.matrices.multiply(Matrix::from_4x3(params)),
            MTX_MULT_3X3 => self.matrices.multiply(Matrix::from_3x3(params)),
            MTX_SCALE => self.matrices.scale(Matrix::scale(params)),
            MTX_TRANS => self.matrices.multiply(Matrix::translation(params)),
            COLOR => self.color = rgb(param),
            TEXCOORD => {
                let mode = self.texcoord_mode();
                self.texcoords.set(param, mode, self.matrices.texture());
            }
            VTX_16 => {
                self.vertex = [param as i16, (param >> 16) as i16, params[1] as i16];
                self.submit_vertex();
            }
            VTX_10 => {
                self.vertex = [0, 10, 20].map(|shift| ten_bits(param, shift));
                self.submit_vertex();
            }
            VTX_XY => {
                [self.vertex[0], self.vertex[1]] = halves(param);
                self.submit_vertex();
            }
            VTX_XZ => {
                [self.vertex[0], self.vertex[2]] = halves(param);
                self.submit_vertex();
            }
            VTX_YZ => {
                [self.vertex[1], self.vertex[2]] = halves(param);
                self.submit_vertex();
            }
            VTX_DIFF => {
                for (axis, shift) in [0, 10, 20].into_iter().enumerate() {
                    let delta = ten_bits(param, shift) >> 6;
                    self.vertex[axis] = self.vertex[axis].wrapping_add(delta);
                }
                self.submit_vertex();
            }
            POLYGON_ATTR => self.polygons.set_attributes(param),
            TEXIMAGE_PARAM => self.polygons.set_texture(param),
            PLTT_BASE => self.polygons.set_palette(param),
            NORMAL => {
                let normal = light::unpack_vector(param);
                let mode = self.texcoord_mode();
                self.texcoords.normal(normal, mode, self.matrices.texture());
                let lights = self.polygons.attributes() & LIGHTS;
                self.color = self.lighting.shade(normal, self.matrices.vector(), lights);
            }
            DIF_AMB => {
                if let Some(color) = self.lighting.set_diffuse_ambient(param) {
                    self.color = color;
                }
            }
            SPE_EMI => self.lighting.set_specular_emission(param),
            LIGHT_VECTOR => self.lighting.set_direction(param, self.matrices.vector()),
            LIGHT_COLOR => self.lighting.set_color(param),
            SHININESS => self.lighting.set_table(params),
            BEGIN_VTXS => self.polygons.begin(param),
            SWAP_BUFFERS => self.polygons.request_swap(param),
            VIEWPORT => self.polygons.set_viewport(param),
            BOX_TEST => {
                let keep_far = self.polygons.attributes() & RENDER_FAR != 0;
                self.box_inside = query::inside(params, self.matrices.clip(), keep_far);
            }
            POS_TEST => {
                self.vertex = [param as i16, (param >> 16) as i16, params[1] as i16];
                self.position_result = query::position(self.vertex, self.matrices.clip());
            }
            VEC_TEST => self.vector_result = query::direction(param, self.matrices.vector()),
            VBLANK => {
                self.polygons.swap();
            }
            ACKNOWLEDGE_STACK => self.matrices.acknowledge_error(),
            ACKNOWLEDGE_OVERFLOW => self.polygons.acknowledge_overflow(),
            SET_DOT_DEPTH => self.polygons.set_dot_depth(param),
            _ => {}
        }
    }

    fn submit_vertex(&mut self) {
        let [x, y, z] = self.vertex.map(i64::from);
        let position = self
            .matrices
            .clip()
            .transform([x, y, z, i64::from(matrix::ONE)]);
        let texcoord =
            self.texcoords
                .vertex(self.vertex, self.texcoord_mode(), self.matrices.texture());
        self.polygons
            .vertex(Point::new(position, self.color, texcoord));
    }

    fn texcoord_mode(&self) -> texcoord::Mode {
        texcoord::Mode::from_texture(self.polygons.texture())
    }
}

fn rgb(param: u32) -> [u8; 3] {
    [0, 5, 10].map(|shift| ((param >> shift) & COLOR_MASK) as u8)
}

fn ten_bits(param: u32, shift: u32) -> i16 {
    ((((param >> shift) & 0x3FF) << 6) as u16) as i16
}

fn halves(param: u32) -> [i16; 2] {
    [param as i16, (param >> 16) as i16]
}
