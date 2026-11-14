pub const ONE: i32 = 0x1000;

const FRACTION: u32 = 12;
const POSITION_ENTRIES: usize = 32;
const POSITION_VALID: u8 = 30;
const POINTER_MASK: u8 = 0x3F;
const ENTRY_MASK: u8 = 0x1F;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Matrix(pub [i32; 16]);

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Self = Self([ONE, 0, 0, 0, 0, ONE, 0, 0, 0, 0, ONE, 0, 0, 0, 0, ONE]);

    pub fn from_4x4(params: &[u32]) -> Self {
        let mut m = [0; 16];
        for (cell, &param) in m.iter_mut().zip(params) {
            *cell = param as i32;
        }
        Self(m)
    }

    pub fn from_4x3(params: &[u32]) -> Self {
        let mut m = Self::IDENTITY.0;
        for row in 0..4 {
            for col in 0..3 {
                m[row * 4 + col] = params[row * 3 + col] as i32;
            }
            m[row * 4 + 3] = if row == 3 { ONE } else { 0 };
        }
        Self(m)
    }

    pub fn from_3x3(params: &[u32]) -> Self {
        let mut m = Self::IDENTITY.0;
        for row in 0..3 {
            for col in 0..3 {
                m[row * 4 + col] = params[row * 3 + col] as i32;
            }
        }
        Self(m)
    }

    pub fn scale(params: &[u32]) -> Self {
        let mut m = Self::IDENTITY.0;
        for (axis, &param) in params.iter().take(3).enumerate() {
            m[axis * 5] = param as i32;
        }
        Self(m)
    }

    pub fn translation(params: &[u32]) -> Self {
        let mut m = Self::IDENTITY.0;
        for (axis, &param) in params.iter().take(3).enumerate() {
            m[12 + axis] = param as i32;
        }
        Self(m)
    }

    pub fn times(&self, rhs: &Self) -> Self {
        let mut m = [0; 16];
        for row in 0..4 {
            for col in 0..4 {
                let sum = (0..4).fold(0i64, |sum, k| {
                    sum.wrapping_add(i64::from(self.0[row * 4 + k]) * i64::from(rhs.0[k * 4 + col]))
                });
                m[row * 4 + col] = (sum >> FRACTION) as i32;
            }
        }
        Self(m)
    }

    pub fn transform(&self, vector: [i64; 4]) -> [i32; 4] {
        let mut out = [0; 4];
        for (col, cell) in out.iter_mut().enumerate() {
            let sum = (0..4).fold(0i64, |sum, k| {
                sum.wrapping_add(vector[k].wrapping_mul(i64::from(self.0[k * 4 + col])))
            });
            *cell = (sum >> FRACTION) as i32;
        }
        out
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Mode {
    #[default]
    Projection,
    Position,
    PositionVector,
    Texture,
}

impl Mode {
    pub fn from_param(param: u32) -> Self {
        match param & 3 {
            0 => Self::Projection,
            1 => Self::Position,
            2 => Self::PositionVector,
            _ => Self::Texture,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Stack {
    mode: Mode,
    projection: u8,
    position: u8,
    texture: u8,
    error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Projection,
    Texture,
    Position(usize),
}

impl Stack {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_mode(&mut self, param: u32) {
        self.mode = Mode::from_param(param);
    }

    pub fn position_pointer(&self) -> u8 {
        self.position
    }

    pub fn projection_pointer(&self) -> u8 {
        self.projection
    }

    pub fn error(&self) -> bool {
        self.error
    }

    pub fn acknowledge_error(&mut self) {
        self.error = false;
        self.projection = 0;
        self.texture = 0;
    }

    pub fn push(&mut self) -> Slot {
        match self.mode {
            Mode::Projection => {
                self.error |= self.projection > 0;
                self.projection = (self.projection + 1) & 1;
                Slot::Projection
            }
            Mode::Texture => {
                self.error |= self.texture > 0;
                self.texture = (self.texture + 1) & 1;
                Slot::Texture
            }
            Mode::Position | Mode::PositionVector => {
                self.error |= self.position > POSITION_VALID;
                let entry = usize::from(self.position & ENTRY_MASK);
                self.position = (self.position + 1) & POINTER_MASK;
                Slot::Position(entry)
            }
        }
    }

    pub fn pop(&mut self, param: u32) -> Slot {
        match self.mode {
            Mode::Projection => {
                self.error |= self.projection == 0;
                self.projection = self.projection.wrapping_sub(1) & 1;
                Slot::Projection
            }
            Mode::Texture => {
                self.error |= self.texture == 0;
                self.texture = self.texture.wrapping_sub(1) & 1;
                Slot::Texture
            }
            Mode::Position | Mode::PositionVector => {
                let offset = (((param as u8) << 2) as i8) >> 2;
                self.position = self.position.wrapping_sub(offset as u8) & POINTER_MASK;
                self.error |= self.position > POSITION_VALID;
                Slot::Position(usize::from(self.position & ENTRY_MASK))
            }
        }
    }

    pub fn store(&mut self, param: u32) -> Slot {
        self.address(param)
    }

    pub fn restore(&mut self, param: u32) -> Slot {
        self.address(param)
    }

    fn address(&mut self, param: u32) -> Slot {
        match self.mode {
            Mode::Projection => Slot::Projection,
            Mode::Texture => Slot::Texture,
            Mode::Position | Mode::PositionVector => {
                let entry = param as u8 & ENTRY_MASK;
                self.error |= entry > POSITION_VALID;
                Slot::Position(usize::from(entry))
            }
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Matrices {
    stack: Stack,
    projection: Matrix,
    position: Matrix,
    vector: Matrix,
    texture: Matrix,
    clip: Matrix,
    projection_stack: Matrix,
    position_stack: [Matrix; POSITION_ENTRIES],
    vector_stack: [Matrix; POSITION_ENTRIES],
    texture_stack: Matrix,
}

impl Matrices {
    pub fn set_mode(&mut self, param: u32) {
        self.stack.set_mode(param);
    }

    pub fn clip(&self) -> &Matrix {
        &self.clip
    }

    pub fn vector(&self) -> &Matrix {
        &self.vector
    }

    pub fn texture(&self) -> &Matrix {
        &self.texture
    }

    pub fn acknowledge_error(&mut self) {
        self.stack.acknowledge_error();
    }

    pub fn identity(&mut self) {
        self.apply(|_| Matrix::IDENTITY, true);
    }

    pub fn load(&mut self, matrix: Matrix) {
        self.apply(|_| matrix, true);
    }

    pub fn multiply(&mut self, matrix: Matrix) {
        self.apply(|current| matrix.times(current), true);
    }

    pub fn scale(&mut self, matrix: Matrix) {
        self.apply(|current| matrix.times(current), false);
    }

    pub fn push(&mut self) {
        let slot = self.stack.push();
        self.save(slot);
    }

    pub fn pop(&mut self, param: u32) {
        let slot = self.stack.pop(param);
        self.take(slot);
    }

    pub fn store(&mut self, param: u32) {
        let slot = self.stack.store(param);
        self.save(slot);
    }

    pub fn restore(&mut self, param: u32) {
        let slot = self.stack.restore(param);
        self.take(slot);
    }

    fn save(&mut self, slot: Slot) {
        match slot {
            Slot::Projection => self.projection_stack = self.projection,
            Slot::Texture => self.texture_stack = self.texture,
            Slot::Position(entry) => {
                self.position_stack[entry] = self.position;
                self.vector_stack[entry] = self.vector;
            }
        }
    }

    fn take(&mut self, slot: Slot) {
        match slot {
            Slot::Projection => {
                self.projection = self.projection_stack;
                self.update_clip();
            }
            Slot::Texture => self.texture = self.texture_stack,
            Slot::Position(entry) => {
                self.position = self.position_stack[entry];
                self.vector = self.vector_stack[entry];
                self.update_clip();
            }
        }
    }

    fn apply(&mut self, change: impl Fn(&Matrix) -> Matrix, vector_too: bool) {
        match self.stack.mode {
            Mode::Projection => {
                self.projection = change(&self.projection);
                self.update_clip();
            }
            Mode::Position => {
                self.position = change(&self.position);
                self.update_clip();
            }
            Mode::PositionVector => {
                self.position = change(&self.position);
                if vector_too {
                    self.vector = change(&self.vector);
                }
                self.update_clip();
            }
            Mode::Texture => self.texture = change(&self.texture),
        }
    }

    fn update_clip(&mut self) {
        self.clip = self.position.times(&self.projection);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrices(mode: Mode) -> Matrices {
        Matrices {
            stack: Stack {
                mode,
                ..Stack::default()
            },
            ..Matrices::default()
        }
    }

    fn scaled(value: i32) -> Matrix {
        let v = value as u32;
        Matrix::scale(&[v, v, v])
    }

    #[test]
    fn identity_is_one_on_the_diagonal() {
        assert_eq!(Matrix::IDENTITY.0[0], 0x1000);
        assert_eq!(Matrix::IDENTITY.0[5], 0x1000);
        assert_eq!(Matrix::IDENTITY.0[10], 0x1000);
        assert_eq!(Matrix::IDENTITY.0[15], 0x1000);
        assert_eq!(Matrix::IDENTITY.0.iter().filter(|&&v| v == 0).count(), 12);
    }

    #[test]
    fn four_by_three_fills_the_last_column_from_gbatek() {
        let params: Vec<u32> = (1..=12).collect();
        let m = Matrix::from_4x3(&params);
        assert_eq!(
            m.0,
            [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0, 10, 11, 12, 0x1000]
        );
    }

    #[test]
    fn three_by_three_keeps_the_bottom_row_from_gbatek() {
        let params: Vec<u32> = (1..=9).collect();
        let m = Matrix::from_3x3(&params);
        assert_eq!(m.0, [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0, 0, 0, 0, 0x1000]);
    }

    #[test]
    fn translation_sits_in_the_bottom_row() {
        let m = Matrix::translation(&[5, 6, 7]);
        assert_eq!(&m.0[12..], &[5, 6, 7, 0x1000]);
        assert_eq!(m.0[0], 0x1000);
    }

    #[test]
    fn multiply_floors_the_sum_of_products_like_melonds() {
        let half = Matrix::scale(&[0x0800, 0x0800, 0x0800]);
        let odd = Matrix::scale(&[0x0001, 0xFFFF_FFFF, 0x0003]);
        let m = half.times(&odd);
        assert_eq!(m.0[0], 0);
        assert_eq!(m.0[5], -1);
        assert_eq!(m.0[10], 1);
    }

    #[test]
    fn multiply_sums_before_dropping_the_fraction() {
        let a = Matrix::from_4x4(&[
            1, 1, 0, 0, 0, 0x1000, 0, 0, 0, 0, 0x1000, 0, 0, 0, 0, 0x1000,
        ]);
        let b = Matrix::from_4x4(&[
            0x0800, 0, 0, 0, 0x0800, 0, 0, 0, 0, 0, 0x1000, 0, 0, 0, 0, 0x1000,
        ]);
        assert_eq!(a.times(&b).0[0], 1);
    }

    #[test]
    fn multiply_truncates_to_32_bits() {
        let big = Matrix::scale(&[0x4000_0000, 0x1000, 0x1000]);
        let m = big.times(&Matrix::scale(&[0x0001_0000, 0x1000, 0x1000]));
        assert_eq!(m.0[0], 0);
    }

    #[test]
    fn transform_uses_the_row_vector_convention() {
        let m = Matrix::translation(&[0x1000, 0x2000, 0x3000]);
        assert_eq!(
            m.transform([0x1000, 0, 0, 0x1000]),
            [0x2000, 0x2000, 0x3000, 0x1000]
        );
    }

    #[test]
    fn multiply_puts_the_new_matrix_on_the_left() {
        let mut matrices = matrices(Mode::Position);
        matrices.load(Matrix::translation(&[0x1000, 0, 0]));
        matrices.multiply(scaled(0x2000));
        assert_eq!(matrices.position.0[12], 0x1000);
        assert_eq!(matrices.position.0[0], 0x2000);
    }

    #[test]
    fn clip_is_position_times_projection() {
        let mut matrices = matrices(Mode::Projection);
        matrices.load(scaled(0x2000));
        matrices.set_mode(1);
        matrices.load(Matrix::translation(&[0x1000, 0, 0]));
        assert_eq!(matrices.clip().0[12], 0x2000);
        assert_eq!(matrices.clip().0[0], 0x2000);
    }

    #[test]
    fn position_mode_leaves_the_vector_matrix() {
        let mut matrices = matrices(Mode::Position);
        matrices.load(scaled(0x2000));
        assert_eq!(matrices.vector, Matrix::IDENTITY);
    }

    #[test]
    fn position_vector_mode_sets_both() {
        let mut matrices = matrices(Mode::PositionVector);
        matrices.load(scaled(0x2000));
        assert_eq!(matrices.vector, scaled(0x2000));
        assert_eq!(matrices.position, scaled(0x2000));
    }

    #[test]
    fn scale_in_position_vector_mode_leaves_the_vector_matrix_per_gbatek() {
        let mut matrices = matrices(Mode::PositionVector);
        matrices.scale(scaled(0x2000));
        assert_eq!(matrices.position, scaled(0x2000));
        assert_eq!(matrices.vector, Matrix::IDENTITY);
    }

    #[test]
    fn texture_mode_leaves_clip_alone() {
        let mut matrices = matrices(Mode::Texture);
        matrices.load(scaled(0x2000));
        assert_eq!(matrices.texture, scaled(0x2000));
        assert_eq!(matrices.clip, Matrix::IDENTITY);
    }

    #[test]
    fn projection_push_then_pop_restores() {
        let mut matrices = matrices(Mode::Projection);
        matrices.load(scaled(0x2000));
        matrices.push();
        matrices.identity();
        matrices.pop(0);
        assert_eq!(matrices.projection, scaled(0x2000));
        assert_eq!(matrices.stack.projection_pointer(), 0);
        assert!(!matrices.stack.error());
    }

    #[test]
    fn projection_second_push_flags_overflow() {
        let mut matrices = matrices(Mode::Projection);
        matrices.push();
        assert!(!matrices.stack.error());
        matrices.push();
        assert!(matrices.stack.error());
        assert_eq!(matrices.stack.projection_pointer(), 0);
    }

    #[test]
    fn projection_pop_when_empty_flags_underflow() {
        let mut matrices = matrices(Mode::Projection);
        matrices.pop(1);
        assert!(matrices.stack.error());
        assert_eq!(matrices.stack.projection_pointer(), 1);
    }

    #[test]
    fn projection_pop_ignores_its_offset() {
        let mut matrices = matrices(Mode::Projection);
        matrices.push();
        matrices.pop(5);
        assert_eq!(matrices.stack.projection_pointer(), 0);
        assert!(!matrices.stack.error());
    }

    #[test]
    fn position_push_saves_both_matrices() {
        let mut matrices = matrices(Mode::Position);
        matrices.load(scaled(0x2000));
        matrices.set_mode(2);
        matrices.multiply(scaled(0x3000));
        matrices.set_mode(1);
        matrices.push();
        matrices.set_mode(2);
        matrices.identity();
        matrices.set_mode(1);
        matrices.pop(1);
        assert_eq!(matrices.position.0[0], 0x6000);
        assert_eq!(matrices.vector.0[0], 0x3000);
        assert_eq!(matrices.stack.position_pointer(), 0);
    }

    #[test]
    fn position_pop_by_two_drops_an_entry() {
        let mut matrices = matrices(Mode::PositionVector);
        for value in [0x1000, 0x2000, 0x3000] {
            matrices.load(scaled(value));
            matrices.push();
        }
        matrices.pop(2);
        assert_eq!(matrices.stack.position_pointer(), 1);
        assert_eq!(matrices.position, scaled(0x2000));
    }

    #[test]
    fn position_pop_by_a_negative_offset_moves_up() {
        let mut matrices = matrices(Mode::PositionVector);
        for value in [0x1000, 0x2000, 0x3000] {
            matrices.load(scaled(value));
            matrices.push();
        }
        matrices.pop(3);
        matrices.pop(0x3F);
        assert_eq!(matrices.stack.position_pointer(), 1);
        assert_eq!(matrices.position, scaled(0x2000));
        assert!(!matrices.stack.error());
    }

    #[test]
    fn position_stack_has_31_valid_entries() {
        let mut matrices = matrices(Mode::PositionVector);
        for _ in 0..31 {
            matrices.push();
        }
        assert!(!matrices.stack.error());
        assert_eq!(matrices.stack.position_pointer(), 31);
        matrices.push();
        assert!(matrices.stack.error());
        assert_eq!(matrices.stack.position_pointer(), 32);
    }

    #[test]
    fn position_pop_below_zero_flags_and_wraps_to_63() {
        let mut matrices = matrices(Mode::PositionVector);
        matrices.pop(1);
        assert!(matrices.stack.error());
        assert_eq!(matrices.stack.position_pointer(), 63);
    }

    #[test]
    fn position_entry_31_is_kept_though_it_flags_per_gbatek() {
        let mut matrices = matrices(Mode::PositionVector);
        matrices.load(scaled(0x5000));
        matrices.store(31);
        assert!(matrices.stack.error());
        matrices.identity();
        matrices.restore(31);
        assert_eq!(matrices.position, scaled(0x5000));
    }

    #[test]
    fn store_and_restore_leave_the_pointer() {
        let mut matrices = matrices(Mode::PositionVector);
        matrices.load(scaled(0x2000));
        matrices.store(7);
        matrices.identity();
        matrices.restore(7);
        assert_eq!(matrices.position, scaled(0x2000));
        assert_eq!(matrices.stack.position_pointer(), 0);
        assert!(!matrices.stack.error());
    }

    #[test]
    fn store_uses_five_address_bits() {
        let mut matrices = matrices(Mode::PositionVector);
        matrices.load(scaled(0x2000));
        matrices.store(0x23);
        matrices.identity();
        matrices.restore(3);
        assert_eq!(matrices.position, scaled(0x2000));
    }

    #[test]
    fn restore_recomputes_the_clip_matrix() {
        let mut matrices = matrices(Mode::PositionVector);
        matrices.load(scaled(0x2000));
        matrices.store(0);
        matrices.identity();
        matrices.restore(0);
        assert_eq!(matrices.clip, scaled(0x2000));
    }

    #[test]
    fn texture_stack_has_one_entry() {
        let mut matrices = matrices(Mode::Texture);
        matrices.load(scaled(0x2000));
        matrices.push();
        matrices.identity();
        matrices.pop(1);
        assert_eq!(matrices.texture, scaled(0x2000));
        assert!(!matrices.stack.error());
        matrices.pop(1);
        assert!(matrices.stack.error());
    }

    #[test]
    fn acknowledging_resets_the_projection_pointer_per_gbatek() {
        let mut matrices = matrices(Mode::Projection);
        matrices.push();
        matrices.set_mode(2);
        matrices.pop(1);
        assert!(matrices.stack.error());
        matrices.acknowledge_error();
        assert!(!matrices.stack.error());
        assert_eq!(matrices.stack.projection_pointer(), 0);
        assert_eq!(matrices.stack.position_pointer(), 63);
    }
}
