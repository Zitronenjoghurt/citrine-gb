use super::clip::{Points, clip};
use super::light::unpack_vector;
use super::matrix::{Matrix, ONE};
use super::polygon::Point;

const FACES: [[usize; 4]; 6] = [
    [0, 1, 2, 3],
    [4, 5, 6, 7],
    [0, 3, 4, 5],
    [1, 2, 7, 6],
    [0, 1, 6, 5],
    [2, 3, 4, 7],
];
const VECTOR_SHIFT: u32 = 9;
const VECTOR_BITS: u32 = 13;

pub fn position(vertex: [i16; 3], clip: &Matrix) -> [i32; 4] {
    let [x, y, z] = vertex.map(i64::from);
    clip.transform([x, y, z, i64::from(ONE)])
}

pub fn inside(params: &[u32], clip_matrix: &Matrix, keep_far: bool) -> bool {
    let half = |param: u32| [param as i16, (param >> 16) as i16];
    let [x0, y0] = half(params[0]);
    let [z0, width] = half(params[1]);
    let [height, depth] = half(params[2]);
    let (x1, y1, z1) = (
        x0.wrapping_add(width),
        y0.wrapping_add(height),
        z0.wrapping_add(depth),
    );
    let corners = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y1, z1],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
    ]
    .map(|corner| Point::new(position(corner, clip_matrix), [0; 3], [0; 2]));
    FACES.iter().any(|face| {
        let mut points = Points::from_slice(&face.map(|corner| corners[corner]));
        clip(&mut points, 0, keep_far);
        !points.is_empty()
    })
}

pub fn direction(param: u32, vector: &Matrix) -> [i16; 3] {
    let raw = unpack_vector(param);
    [0, 1, 2].map(|axis| {
        let sum = (0..3).fold(0i32, |sum, row| {
            sum.wrapping_add(raw[row].wrapping_mul(vector.0[row * 4 + axis]))
        });
        let level = sum >> VECTOR_SHIFT;
        (level.wrapping_shl(32 - VECTOR_BITS) >> (32 - VECTOR_BITS)) as i16
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_U: u32 = ONE as u32;

    fn cube(x: i16, y: i16, z: i16, size: i16) -> [u32; 3] {
        let pack = |low: i16, high: i16| u32::from(low as u16) | u32::from(high as u16) << 16;
        [pack(x, y), pack(z, size), pack(size, size)]
    }

    #[test]
    fn position_test_multiplies_by_the_clip_matrix_per_gbatek() {
        let clip = Matrix::translation(&[ONE_U, 2 * ONE_U, 0]);
        assert_eq!(
            position([0x800, 0, -0x1000], &clip),
            [0x1800, 0x2000, -0x1000, 0x1000]
        );
    }

    #[test]
    fn a_box_around_the_middle_is_inside() {
        assert!(inside(
            &cube(-0x800, -0x800, -0x800, 0x1000),
            &Matrix::IDENTITY,
            false
        ));
    }

    #[test]
    fn a_box_off_to_the_side_is_outside() {
        assert!(!inside(
            &cube(0x2000, 0, 0, 0x800),
            &Matrix::IDENTITY,
            false
        ));
    }

    #[test]
    fn a_box_reaching_into_the_view_is_inside() {
        assert!(inside(&cube(0x0C00, 0, 0, 0x800), &Matrix::IDENTITY, false));
    }

    #[test]
    fn a_box_holding_the_whole_view_volume_is_outside_per_gbatek() {
        assert!(!inside(
            &cube(-0x3000, -0x3000, -0x3000, 0x6000),
            &Matrix::IDENTITY,
            true
        ));
    }

    #[test]
    fn a_box_crossing_the_far_plane_needs_bit_12_per_gbatek() {
        let params = [0xFC00_0800, 0x1000_D000, 0x4800_0800];
        assert!(!inside(&params, &Matrix::IDENTITY, false));
        assert!(inside(&params, &Matrix::IDENTITY, true));
    }

    #[test]
    fn box_sizes_add_in_16_bits_like_melonds() {
        let params = cube(0x7000, 0, 0, 0x2000);
        assert!(inside(&params, &Matrix::IDENTITY, true));
    }

    #[test]
    fn the_box_goes_through_the_clip_matrix() {
        let away = Matrix::translation(&[4 * ONE_U, 0, 0]);
        assert!(!inside(&cube(-0x800, -0x800, -0x800, 0x1000), &away, false));
    }

    #[test]
    fn vector_test_multiplies_by_the_vector_matrix_per_gbatek() {
        let scale = Matrix::scale(&[0x1800, 0x1000, 0xFFFF_F000]);
        assert_eq!(
            direction(0x100 | 0x080 << 10 | 0x100 << 20, &scale),
            [0x0C00, 0x0400, -0x0800]
        );
    }

    #[test]
    fn vector_test_has_four_sign_bits_so_one_reads_as_minus_one_per_gbatek() {
        let scale = Matrix::scale(&[0x2000, 0x4000, 0x4000]);
        let result = direction(0x100 | 0x0C0 << 10 | 0x140 << 20, &scale);
        assert_eq!(result.map(|axis| axis as u16), [0xF000, 0xF800, 0x0800]);
    }
}
