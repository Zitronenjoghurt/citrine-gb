use super::matrix::Matrix;

const MODE_SHIFT: u32 = 30;
const TEXCOORD_SHIFT: u32 = 12;
const NORMAL_SHIFT: u32 = 21;
const VERTEX_SHIFT: u32 = 24;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Mode {
    #[default]
    None,
    Texcoord,
    Normal,
    Vertex,
}

impl Mode {
    pub fn from_texture(texture: u32) -> Self {
        match texture >> MODE_SHIFT {
            0 => Self::None,
            1 => Self::Texcoord,
            2 => Self::Normal,
            _ => Self::Vertex,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Texcoords {
    raw: [i16; 2],
    current: [i16; 2],
}

impl Texcoords {
    pub fn set(&mut self, param: u32, mode: Mode, matrix: &Matrix) {
        self.raw = [param as i16, (param >> 16) as i16];
        self.current = if mode == Mode::Texcoord {
            let [s, t] = self.raw.map(i32::from);
            [0, 1].map(|axis| {
                let sum = s
                    .wrapping_mul(matrix.0[axis])
                    .wrapping_add(t.wrapping_mul(matrix.0[4 + axis]))
                    .wrapping_add(matrix.0[8 + axis])
                    .wrapping_add(matrix.0[12 + axis]);
                (sum >> TEXCOORD_SHIFT) as i16
            })
        } else {
            self.raw
        };
    }

    pub fn normal(&mut self, normal: [i32; 3], mode: Mode, matrix: &Matrix) {
        if mode == Mode::Normal {
            self.current = self.offset(normal.map(i64::from), matrix, NORMAL_SHIFT);
        }
    }

    pub fn vertex(&self, vertex: [i16; 3], mode: Mode, matrix: &Matrix) -> [i16; 2] {
        if mode == Mode::Vertex {
            self.offset(vertex.map(i64::from), matrix, VERTEX_SHIFT)
        } else {
            self.current
        }
    }

    fn offset(&self, source: [i64; 3], matrix: &Matrix, shift: u32) -> [i16; 2] {
        [0, 1].map(|axis| {
            let sum = (0..3).fold(0i64, |sum, row| {
                sum.wrapping_add(source[row] * i64::from(matrix.0[row * 4 + axis]))
            });
            ((sum >> shift) as i16).wrapping_add(self.raw[axis])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXEL: u32 = 16;

    fn texcoords(param: u32, mode: Mode, matrix: &Matrix) -> Texcoords {
        let mut texcoords = Texcoords::default();
        texcoords.set(param, mode, matrix);
        texcoords
    }

    fn matrix(cells: &[(usize, i32)]) -> Matrix {
        let mut matrix = Matrix::IDENTITY;
        for &(index, value) in cells {
            matrix.0[index] = value;
        }
        matrix
    }

    #[test]
    fn mode_sits_in_the_top_two_bits_of_teximage_param() {
        assert_eq!(Mode::from_texture(0x3FFF_FFFF), Mode::None);
        assert_eq!(Mode::from_texture(1 << 30), Mode::Texcoord);
        assert_eq!(Mode::from_texture(2 << 30), Mode::Normal);
        assert_eq!(Mode::from_texture(3 << 30), Mode::Vertex);
    }

    #[test]
    fn mode_0_passes_the_coordinates_through() {
        let scale = matrix(&[(0, 0x2000)]);
        let texcoords = texcoords(0xFFF0_0020, Mode::None, &scale);
        assert_eq!(
            texcoords.vertex([0x1000; 3], Mode::None, &scale),
            [0x20, -0x10]
        );
    }

    #[test]
    fn mode_1_multiplies_by_the_texture_matrix() {
        let scale = matrix(&[(0, 0x2000), (5, 0x0800)]);
        let texcoords = texcoords(((2 * TEXEL) << 16) | (3 * TEXEL), Mode::Texcoord, &scale);
        assert_eq!(texcoords.current, [6 * TEXEL as i16, TEXEL as i16]);
    }

    #[test]
    fn mode_1_adds_a_sixteenth_of_rows_2_and_3_per_gbatek() {
        let shift = matrix(&[(8, 0x8000), (12, 0x8000), (13, -0x10000)]);
        let texcoords = texcoords(0, Mode::Texcoord, &shift);
        assert_eq!(texcoords.current, [TEXEL as i16, -(TEXEL as i16)]);
    }

    #[test]
    fn mode_1_wraps_to_16_bits_per_gbatek() {
        let big = matrix(&[(0, 0x10_0000)]);
        let texcoords = texcoords(0x7FFF, Mode::Texcoord, &big);
        assert_eq!(texcoords.current[0], -0x100);
    }

    #[test]
    fn mode_2_adds_the_normal_times_the_matrix_like_melonds() {
        let normal_matrix = matrix(&[(0, 0x10_0000), (9, -0x20_0000)]);
        let mut texcoords = texcoords(TEXEL, Mode::Normal, &normal_matrix);
        assert_eq!(texcoords.current, [TEXEL as i16, 0]);
        texcoords.normal([0x100, 0, 0x100], Mode::Normal, &normal_matrix);
        assert_eq!(texcoords.current, [TEXEL as i16 + 128, -256]);
        assert_eq!(
            texcoords.vertex([0; 3], Mode::Normal, &normal_matrix),
            [144, -256]
        );
    }

    #[test]
    fn normal_leaves_other_modes_alone() {
        let mut texcoords = texcoords(TEXEL, Mode::Texcoord, &Matrix::IDENTITY);
        texcoords.normal([0x100, 0, 0], Mode::Texcoord, &matrix(&[(0, 0x10_0000)]));
        assert_eq!(texcoords.current, [TEXEL as i16, 0]);
    }

    #[test]
    fn mode_3_adds_the_vertex_times_the_matrix_like_melonds() {
        let vertex_matrix = matrix(&[(0, 0x1_0000), (5, 0x2_0000), (8, 0x1_0000)]);
        let texcoords = texcoords(TEXEL << 16, Mode::Vertex, &vertex_matrix);
        assert_eq!(
            texcoords.vertex([0x1000, 0x0800, 0x1000], Mode::Vertex, &vertex_matrix),
            [32, 32]
        );
    }
}
