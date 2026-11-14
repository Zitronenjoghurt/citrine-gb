use super::matrix::Matrix;

pub const LIGHTS: usize = 4;

const COLOR_MASK: u32 = 0x1F;
const SET_VERTEX_COLOR: u32 = 1 << 15;
const SHININESS_TABLE: u32 = 1 << 15;
const HIGH_SHIFT: u32 = 16;
const LIGHT_SHIFT: u32 = 30;
const ACCUMULATOR_SHIFT: u32 = 14;
const NORMAL_FRACTION: u32 = 9;
const DIFFUSE_MASK: i32 = 0xF_FFFF;
const SPECULAR_ONE: i32 = 1 << 9;
const SHINE_MAX: i32 = 0x1FF;
const RECIPROCAL_ONE: i32 = 1 << 18;
const MAX_LEVEL: u32 = 31;
const TABLE_WORDS: usize = 32;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lighting {
    diffuse: [u8; 3],
    ambient: [u8; 3],
    specular: [u8; 3],
    emission: [u8; 3],
    use_table: bool,
    table: [u32; TABLE_WORDS],
    directions: [[i32; 3]; LIGHTS],
    reciprocals: [i32; LIGHTS],
    colors: [[u8; 3]; LIGHTS],
}

impl Lighting {
    pub fn set_diffuse_ambient(&mut self, param: u32) -> Option<[u8; 3]> {
        self.diffuse = rgb(param);
        self.ambient = rgb(param >> HIGH_SHIFT);
        (param & SET_VERTEX_COLOR != 0).then_some(self.diffuse)
    }

    pub fn set_specular_emission(&mut self, param: u32) {
        self.specular = rgb(param);
        self.emission = rgb(param >> HIGH_SHIFT);
        self.use_table = param & SHININESS_TABLE != 0;
    }

    pub fn set_table(&mut self, params: &[u32]) {
        for (word, &param) in self.table.iter_mut().zip(params) {
            *word = param;
        }
    }

    pub fn set_color(&mut self, param: u32) {
        self.colors[light(param)] = rgb(param);
    }

    pub fn set_direction(&mut self, param: u32, vector: &Matrix) {
        let raw = unpack_vector(param);
        let product = |axis: usize| dot_column(raw, vector, axis);
        self.directions[light(param)] = [0, 1, 2].map(|axis| {
            let level = product(axis) >> 12;
            sign_extend(level.wrapping_neg(), 11)
        });
        let depth = product(2).wrapping_shl(9) >> 21;
        let divisor = SPECULAR_ONE - depth;
        self.reciprocals[light(param)] = if divisor == 0 {
            0
        } else {
            RECIPROCAL_ONE / divisor
        };
    }

    pub fn shade(&self, normal: [i32; 3], vector: &Matrix, enabled: u32) -> [u8; 3] {
        let normal = [0, 1, 2].map(|axis| dot_column(normal, vector, axis).wrapping_shl(9) >> 21);
        let mut sum = self
            .emission
            .map(|level| u32::from(level) << ACCUMULATOR_SHIFT);
        for index in (0..LIGHTS).filter(|&index| enabled & (1 << index) != 0) {
            let direction = self.directions[index];
            let dot: i32 = (0..3)
                .map(|axis| direction[axis].wrapping_mul(normal[axis]) >> NORMAL_FRACTION)
                .sum();
            let light = self.colors[index].map(i32::from);
            let mut shine = 0;
            if dot > 0 {
                let diffuse_level = sign_extend(dot, 11);
                for channel in 0..3 {
                    let diffuse = i32::from(self.diffuse[channel]) * light[channel];
                    sum[channel] = sum[channel]
                        .wrapping_add((diffuse.wrapping_mul(diffuse_level) & DIFFUSE_MASK) as u32);
                }
                let half = sign_extend(dot + normal[2], 11);
                let squared = ((half * half) >> 10) & 0x3FF;
                shine = ((squared * self.reciprocals[index]) >> 8) - SPECULAR_ONE;
                shine = if shine < 0 {
                    0
                } else {
                    sign_extend(shine, 14).clamp(0, SHINE_MAX)
                };
            }
            if self.use_table {
                shine = i32::from(self.table_entry((shine >> 2) as usize)) << 1;
            }
            for channel in 0..3 {
                let specular = i32::from(self.specular[channel]) * shine;
                let ambient = i32::from(self.ambient[channel]) << 9;
                sum[channel] = sum[channel]
                    .wrapping_add((specular + ambient).wrapping_mul(light[channel]) as u32);
            }
        }
        sum.map(|level| (level >> ACCUMULATOR_SHIFT).min(MAX_LEVEL) as u8)
    }

    fn table_entry(&self, index: usize) -> u8 {
        (self.table[index / 4] >> ((index % 4) * 8)) as u8
    }
}

pub fn unpack_vector(param: u32) -> [i32; 3] {
    [0, 10, 20].map(|shift| sign_extend(((param >> shift) & 0x3FF) as i32, 10))
}

fn dot_column(vector: [i32; 3], matrix: &Matrix, axis: usize) -> i32 {
    (0..3).fold(0i32, |sum, row| {
        sum.wrapping_add(vector[row].wrapping_mul(matrix.0[row * 4 + axis]))
    })
}

fn sign_extend(value: i32, bits: u32) -> i32 {
    value.wrapping_shl(32 - bits) >> (32 - bits)
}

fn light(param: u32) -> usize {
    (param >> LIGHT_SHIFT) as usize
}

fn rgb(param: u32) -> [u8; 3] {
    [0, 5, 10].map(|shift| ((param >> shift) & COLOR_MASK) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: u32 = 0x7FFF;
    const INTO_SCREEN: u32 = 0x201 << 20;
    const OUT_OF_SCREEN: u32 = 0x1FF << 20;
    const LIGHT_0: u32 = 1;

    fn lit(diffuse_ambient: u32, specular_emission: u32) -> Lighting {
        let mut lighting = Lighting::default();
        lighting.set_diffuse_ambient(diffuse_ambient);
        lighting.set_specular_emission(specular_emission);
        lighting.set_color(WHITE);
        lighting.set_direction(INTO_SCREEN, &Matrix::IDENTITY);
        lighting
    }

    fn facing_viewer() -> [i32; 3] {
        unpack_vector(OUT_OF_SCREEN)
    }

    #[test]
    fn vectors_are_three_signed_ten_bit_fields() {
        assert_eq!(
            unpack_vector(0x1FF | 0x200 << 10 | 0x3FF << 20),
            [511, -512, -1]
        );
    }

    #[test]
    fn emission_alone_without_lights_per_gbatek() {
        let lighting = lit(WHITE, 0x1234 << 16);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, 0),
            [20, 17, 4]
        );
    }

    #[test]
    fn diffuse_from_a_light_shining_straight_on() {
        let lighting = lit(WHITE, 0);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [29, 29, 29]
        );
    }

    #[test]
    fn diffuse_scales_with_the_light_color() {
        let mut lighting = lit(WHITE, 0);
        lighting.set_color(16 | 8 << 5);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [15, 7, 0]
        );
    }

    #[test]
    fn a_light_behind_the_surface_gives_no_diffuse() {
        let mut lighting = lit(WHITE, 0);
        lighting.set_direction(OUT_OF_SCREEN, &Matrix::IDENTITY);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [0, 0, 0]
        );
    }

    #[test]
    fn a_disabled_light_adds_nothing() {
        let lighting = lit(WHITE | WHITE << 16, 0);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, 0b10),
            [0, 0, 0]
        );
    }

    #[test]
    fn ambient_needs_no_facing_light() {
        let mut lighting = lit(WHITE << 16, 0);
        lighting.set_direction(OUT_OF_SCREEN, &Matrix::IDENTITY);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [30, 30, 30]
        );
    }

    #[test]
    fn each_channel_stops_at_31() {
        let lighting = lit(WHITE << 16, WHITE << 16);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [31, 31, 31]
        );
    }

    #[test]
    fn specular_peaks_with_the_light_on_the_line_of_sight_like_melonds() {
        let lighting = lit(0, WHITE);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [29, 29, 29]
        );
    }

    #[test]
    fn shininess_table_replaces_the_specular_level_per_gbatek() {
        let mut lighting = lit(0, WHITE | SHININESS_TABLE);
        lighting.set_table(&[0x8080_8080; 32]);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [15, 15, 15]
        );
    }

    #[test]
    fn shininess_table_bytes_run_low_to_high_per_gbatek() {
        let mut lighting = lit(0, WHITE | SHININESS_TABLE);
        let mut table = [0u32; 32];
        table[31] = 0x40 << 16;
        lighting.set_table(&table);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [7, 7, 7]
        );
    }

    #[test]
    fn light_vector_goes_through_the_vector_matrix_when_set_per_gbatek() {
        let mut lighting = lit(WHITE, 0);
        let flip = Matrix::scale(&[0x1000, 0x1000, 0xFFFF_F000]);
        lighting.set_direction(INTO_SCREEN, &flip);
        assert_eq!(
            lighting.shade(facing_viewer(), &Matrix::IDENTITY, LIGHT_0),
            [0, 0, 0]
        );
    }

    #[test]
    fn normal_goes_through_the_vector_matrix_per_gbatek() {
        let lighting = lit(WHITE, 0);
        let flip = Matrix::scale(&[0x1000, 0x1000, 0xFFFF_F000]);
        assert_eq!(
            lighting.shade(unpack_vector(INTO_SCREEN), &flip, LIGHT_0),
            [29, 29, 29]
        );
    }

    #[test]
    fn bit_15_of_diffuse_sets_the_vertex_color_per_gbatek() {
        let mut lighting = Lighting::default();
        assert_eq!(lighting.set_diffuse_ambient(3 | 4 << 5), None);
        assert_eq!(
            lighting.set_diffuse_ambient(3 | 4 << 5 | SET_VERTEX_COLOR),
            Some([3, 4, 0])
        );
    }

    #[test]
    fn light_number_sits_in_the_top_two_bits() {
        let mut lighting = Lighting::default();
        lighting.set_color(3 << 30 | 9);
        assert_eq!(lighting.colors[3], [9, 0, 0]);
        assert_eq!(lighting.colors[0], [0, 0, 0]);
    }
}
