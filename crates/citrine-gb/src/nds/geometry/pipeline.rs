use super::command::*;
use super::matrix::Mode;

const VERTEX_LENGTH: u32 = 7;
const VERTEX_WORK: u32 = 3;
const NORMAL_LENGTH: u32 = 7;
const SLOT_CYCLES: u32 = 9;
const FIRST_SLOT: u32 = 1;
const CULLED: u32 = 8;
const CULLED_SLOTS: u32 = 0b11110;
const TRIANGLE: u32 = 26;
const TRIANGLE_SLOTS: u32 = 0b1110;
const TRIANGLE_STRIP_SLOTS: u32 = 0b1000;
const QUAD: u32 = 35;
const QUAD_SLOTS: u32 = 0b11110;
const QUAD_STRIP_SLOTS: u32 = 0b11100;
const SWAP_CYCLES: u32 = 325;
const PUSH_WORK: u32 = 16;
const POP_WORK: u32 = 35;
const TEXTURE_POP_WORK: u32 = 17;
const IDENTITY_WORK: u32 = 18;
const LOAD_WORK: u32 = 18;
const TEXTURE_LOAD_4X4_WORK: u32 = 10;
const TEXTURE_LOAD_4X3_WORK: u32 = 7;
const MULTIPLY_WORK: u32 = 35;
const TEXTURE_MULTIPLY_WORK: u32 = 33;
const VECTOR_MULTIPLY_WORK: u32 = 30;
const MATERIAL_WORK: u32 = 3;
const LIGHT_VECTOR_WORK: u32 = 5;
const LIGHT_VECTOR_STALL: u32 = 9;
const LIGHT_COLOR_WORK: u32 = 1;
const BEGIN_STALL: u32 = 1;
const BOX_TEST_STALL: u32 = 11;
const BOX_TEST_WORK: u32 = 254;
const POS_TEST_WORK: u32 = 5;
const VEC_TEST_WORK: u32 = 4;
const QUADS: u8 = 1;
const STRIPS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pipelines {
    vertex: u32,
    normal: u32,
    polygon: u32,
    slot_clock: u32,
    free_slots: u32,
    polygon_mode: u8,
    vertices: u8,
    culling: Culling,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Culling {
    built: u32,
    submitted: u32,
    seen: u32,
    credit: u32,
}

impl Culling {
    fn next_built(&mut self) -> bool {
        self.seen = self.seen.saturating_add(1);
        if self.submitted == 0 {
            return true;
        }
        self.credit += self.built;
        if self.credit < self.submitted {
            return false;
        }
        self.credit -= self.submitted;
        true
    }

    fn close_frame(&mut self, built: u32) {
        let submitted = self.seen;
        *self = Self {
            built: built.min(submitted),
            submitted,
            seen: 0,
            credit: submitted / 2,
        };
    }
}

impl Default for Pipelines {
    fn default() -> Self {
        Self {
            vertex: 0,
            normal: 0,
            polygon: 0,
            slot_clock: 0,
            free_slots: FIRST_SLOT,
            polygon_mode: 0,
            vertices: 0,
            culling: Culling::default(),
        }
    }
}

impl Pipelines {
    #[inline(always)]
    pub fn charge(
        &mut self,
        command: u8,
        param: u32,
        taken: usize,
        mode: Mode,
        lights: u32,
    ) -> u32 {
        let needed = parameters(command);
        if needed <= 1 {
            return self.single(command, param, mode, lights);
        }
        if taken == 0 {
            return match command {
                VTX_16 => self.vertex_slot(),
                SHININESS | POS_TEST => self.after_vertex(8),
                BOX_TEST => self.stall(BOX_TEST_STALL, 8),
                _ => self.after_vertex(4),
            };
        }
        let param_cost = self.add(1);
        if taken + 1 < needed {
            return param_cost;
        }
        param_cost
            + match command {
                MTX_LOAD_4X4..=MTX_TRANS => self.add(matrix_work(command, mode)),
                VTX_16 => self.vertex(),
                POS_TEST => self.add(POS_TEST_WORK),
                BOX_TEST => self.add(BOX_TEST_WORK),
                _ => 0,
            }
    }

    pub fn idle(&mut self, cycles: u32) {
        self.add(cycles);
        self.normal = self.normal.saturating_sub(cycles);
    }

    pub fn close_frame(&mut self, built: u32) {
        self.culling.close_frame(built);
    }

    pub fn drain(&self) -> u32 {
        self.vertex.max(self.normal).max(self.polygon)
    }

    pub fn active(&self) -> bool {
        self.drain() > 0
    }

    #[inline(always)]
    fn single(&mut self, command: u8, param: u32, mode: Mode, lights: u32) -> u32 {
        let texture = mode == Mode::Texture;
        match command {
            MTX_PUSH | MTX_STORE => self.after_vertex(4) + self.add(PUSH_WORK),
            MTX_POP | MTX_RESTORE if texture => self.after_vertex(4) + self.add(TEXTURE_POP_WORK),
            MTX_POP | MTX_RESTORE => self.after_vertex(4) + self.add(POP_WORK),
            MTX_IDENTITY if texture => self.after_vertex(4),
            MTX_IDENTITY => self.after_vertex(4) + self.add(IDENTITY_WORK),
            COLOR => self.after_vertex(6),
            NORMAL => self.after_vertex(4) + self.light(lights),
            VTX_10..=VTX_DIFF => self.vertex_slot() + self.vertex(),
            POLYGON_ATTR | TEXIMAGE_PARAM | PLTT_BASE | END_VTXS | VIEWPORT => self.after_vertex(8),
            DIF_AMB | SPE_EMI => self.after_vertex(6) + self.add(MATERIAL_WORK),
            LIGHT_VECTOR => self.stall(LIGHT_VECTOR_STALL, 6) + self.add(LIGHT_VECTOR_WORK),
            LIGHT_COLOR => self.after_vertex(8) + self.add(LIGHT_COLOR_WORK),
            BEGIN_VTXS => {
                let cost = self.stall(BEGIN_STALL, 8);
                self.polygon_mode = (param & 3) as u8;
                self.vertices = 0;
                cost
            }
            SWAP_BUFFERS => {
                self.after_vertex(4);
                *self = Self {
                    polygon_mode: self.polygon_mode,
                    vertices: self.vertices,
                    culling: self.culling,
                    ..Self::default()
                };
                SWAP_CYCLES
            }
            VEC_TEST => self.after_vertex(6) + self.add(VEC_TEST_WORK),
            _ => self.after_vertex(4),
        }
    }

    fn add(&mut self, cycles: u32) -> u32 {
        self.vertex = self.vertex.saturating_sub(cycles);
        if self.polygon > cycles {
            self.polygon -= cycles;
            self.slot_clock += cycles;
            while self.slot_clock > SLOT_CYCLES {
                self.slot_clock -= SLOT_CYCLES;
                self.free_slots >>= 1;
            }
        } else if self.polygon > 0 {
            self.end_polygon();
        }
        cycles
    }

    fn end_polygon(&mut self) {
        self.polygon = 0;
        self.slot_clock = 0;
        self.free_slots = FIRST_SLOT;
    }

    fn after_vertex(&mut self, gap: u32) -> u32 {
        let slack = 8 - gap;
        let cost = if self.vertex > slack {
            self.add(self.vertex - slack + 1)
        } else {
            self.add(self.normal + 1)
        };
        self.normal = 0;
        cost
    }

    fn stall(&mut self, delay: u32, gap: u32) -> u32 {
        if self.polygon > 0 {
            let cost = self.polygon + delay;
            self.vertex = 0;
            self.normal = 0;
            self.end_polygon();
            return cost;
        }
        let slack = 8 - gap;
        if self.vertex > slack {
            self.add(self.vertex - slack + 1)
        } else {
            self.add(self.normal + 1)
        }
    }

    fn light(&mut self, lights: u32) -> u32 {
        self.normal = NORMAL_LENGTH;
        self.add(lights.count_ones().max(1))
    }

    fn vertex_slot(&mut self) -> u32 {
        let cost = if self.free_slots & 1 == 0 {
            self.next_slot()
        } else {
            self.add(1)
        };
        self.normal = 0;
        cost
    }

    fn next_slot(&mut self) -> u32 {
        let mut wait = SLOT_CYCLES - self.slot_clock + 1;
        let mut cost = 0;
        loop {
            cost += wait;
            self.vertex = self.vertex.saturating_sub(wait);
            if self.polygon <= wait {
                self.end_polygon();
                return cost;
            }
            self.polygon -= wait;
            self.slot_clock = 1;
            self.free_slots >>= 1;
            if self.free_slots & 1 != 0 {
                self.free_slots &= !1;
                return cost;
            }
            wait = SLOT_CYCLES;
        }
    }

    fn vertex(&mut self) -> u32 {
        self.vertices += 1;
        let (needed, kept) = match (
            self.polygon_mode & QUADS != 0,
            self.polygon_mode & STRIPS != 0,
        ) {
            (false, false) => (3, 0),
            (true, false) => (4, 0),
            (false, true) => (3, 2),
            (true, true) => (4, 2),
        };
        if self.vertices == needed {
            self.vertices = kept;
            self.submit_polygon();
        }
        self.vertex = VERTEX_LENGTH;
        self.add(VERTEX_WORK)
    }

    fn submit_polygon(&mut self) {
        let strips = self.polygon_mode & STRIPS != 0;
        let (length, slots) = match (self.polygon_mode & QUADS != 0, strips) {
            _ if !self.culling.next_built() => (CULLED, CULLED_SLOTS),
            (false, false) => (TRIANGLE, TRIANGLE_SLOTS),
            (false, true) => (TRIANGLE, TRIANGLE_STRIP_SLOTS),
            (true, false) => (QUAD, QUAD_SLOTS),
            (true, true) => (QUAD, QUAD_STRIP_SLOTS),
        };
        self.polygon = length;
        self.slot_clock = 1;
        self.free_slots = slots;
    }
}

fn matrix_work(command: u8, mode: Mode) -> u32 {
    let texture = mode == Mode::Texture;
    match command {
        MTX_LOAD_4X4 if texture => TEXTURE_LOAD_4X4_WORK,
        MTX_LOAD_4X3 if texture => TEXTURE_LOAD_4X3_WORK,
        MTX_LOAD_4X4 | MTX_LOAD_4X3 => LOAD_WORK,
        _ => {
            let multiply = if texture {
                TEXTURE_MULTIPLY_WORK
            } else {
                MULTIPLY_WORK
            };
            let vector = if mode == Mode::PositionVector && command != MTX_SCALE {
                VECTOR_MULTIPLY_WORK
            } else {
                0
            };
            multiply + vector - parameters(command) as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whole(pipelines: &mut Pipelines, command: u8, mode: Mode, lights: u32) -> u32 {
        (0..parameters(command).max(1))
            .map(|taken| pipelines.charge(command, 0, taken, mode, lights))
            .sum()
    }

    fn fresh(command: u8, mode: Mode) -> u32 {
        whole(&mut Pipelines::default(), command, mode, 0)
    }

    #[test]
    fn commands_take_melonds_cycles_when_the_pipelines_are_empty() {
        let expected = [
            (MTX_MODE, 1),
            (MTX_PUSH, 17),
            (MTX_POP, 36),
            (MTX_STORE, 17),
            (MTX_RESTORE, 36),
            (MTX_IDENTITY, 19),
            (MTX_LOAD_4X4, 34),
            (MTX_LOAD_4X3, 30),
            (MTX_MULT_4X4, 35),
            (MTX_MULT_4X3, 35),
            (MTX_MULT_3X3, 35),
            (MTX_SCALE, 35),
            (MTX_TRANS, 35),
            (COLOR, 1),
            (NORMAL, 2),
            (TEXCOORD, 1),
            (VTX_16, 5),
            (VTX_10, 4),
            (VTX_DIFF, 4),
            (POLYGON_ATTR, 1),
            (DIF_AMB, 4),
            (LIGHT_VECTOR, 6),
            (LIGHT_COLOR, 2),
            (SHININESS, 32),
            (BEGIN_VTXS, 1),
            (END_VTXS, 1),
            (SWAP_BUFFERS, SWAP_CYCLES),
            (VIEWPORT, 1),
            (BOX_TEST, 257),
            (POS_TEST, 7),
            (VEC_TEST, 5),
        ];
        for (command, cycles) in expected {
            assert_eq!(
                fresh(command, Mode::Position),
                cycles,
                "command {command:02X}"
            );
        }
    }

    #[test]
    fn simultaneous_mode_adds_30_to_multiplies_and_translate_but_not_scale_or_load() {
        let mode = Mode::PositionVector;
        let cycles: Vec<u32> = [
            MTX_MULT_4X4,
            MTX_MULT_4X3,
            MTX_MULT_3X3,
            MTX_TRANS,
            MTX_SCALE,
            MTX_LOAD_4X4,
        ]
        .map(|command| fresh(command, mode))
        .to_vec();
        assert_eq!(cycles, [65, 65, 65, 65, 35, 34]);
    }

    #[test]
    fn texture_matrix_commands_take_melonds_shorter_times() {
        let mode = Mode::Texture;
        let cycles: Vec<u32> = [
            MTX_IDENTITY,
            MTX_POP,
            MTX_LOAD_4X4,
            MTX_LOAD_4X3,
            MTX_MULT_4X4,
        ]
        .map(|command| fresh(command, mode))
        .to_vec();
        assert_eq!(cycles, [1, 18, 26, 19, 33]);
    }

    #[test]
    fn a_normal_costs_a_cycle_per_light_and_delays_the_next_command_but_not_a_vertex() {
        let mode = Mode::Position;
        let mut pipelines = Pipelines::default();
        let counts: Vec<u32> = (0..=4)
            .map(|lights| whole(&mut Pipelines::default(), NORMAL, mode, (1 << lights) - 1))
            .collect();
        assert_eq!(counts, [2, 2, 3, 4, 5]);
        whole(&mut pipelines, NORMAL, mode, 1);
        assert_eq!(whole(&mut pipelines, TEXCOORD, mode, 1), NORMAL_LENGTH + 1);
        whole(&mut pipelines, NORMAL, mode, 1);
        assert_eq!(whole(&mut pipelines, VTX_10, mode, 1), 4);
    }

    #[test]
    fn a_command_waits_for_the_vertex_pipeline_by_how_soon_it_may_follow_a_vertex() {
        let mode = Mode::Position;
        let after = |command| {
            let mut pipelines = Pipelines::default();
            whole(&mut pipelines, VTX_10, mode, 0);
            whole(&mut pipelines, command, mode, 0)
        };
        assert_eq!(after(TEXCOORD), 1);
        assert_eq!(after(COLOR), 3);
        assert_eq!(after(POLYGON_ATTR), 5);
    }

    #[test]
    fn a_vertex_after_a_triangle_waits_for_a_free_slot_like_melonds() {
        let mode = Mode::Position;
        let mut pipelines = Pipelines::default();
        whole(&mut pipelines, BEGIN_VTXS, mode, 0);
        let cycles: Vec<u32> = (0..6)
            .map(|_| whole(&mut pipelines, VTX_10, mode, 0))
            .collect();
        assert_eq!(cycles, [4, 4, 4, 9, 9, 9]);
    }

    #[test]
    fn begin_vtxs_waits_for_the_polygon_pipeline_to_empty() {
        let mode = Mode::Position;
        let mut pipelines = Pipelines::default();
        whole(&mut pipelines, BEGIN_VTXS, mode, 0);
        for _ in 0..3 {
            whole(&mut pipelines, VTX_10, mode, 0);
        }
        assert_eq!(
            whole(&mut pipelines, BEGIN_VTXS, mode, 0),
            TRIANGLE - VERTEX_WORK + BEGIN_STALL
        );
    }

    #[test]
    fn idle_time_drains_every_pipeline() {
        let mode = Mode::Position;
        let mut pipelines = Pipelines::default();
        whole(&mut pipelines, BEGIN_VTXS, mode, 0);
        for _ in 0..3 {
            whole(&mut pipelines, VTX_10, mode, 0);
        }
        whole(&mut pipelines, NORMAL, mode, 1);
        assert!(pipelines.active());
        pipelines.idle(QUAD);
        assert!(!pipelines.active());
        assert_eq!(whole(&mut pipelines, BEGIN_VTXS, mode, 0), 1);
    }

    #[test]
    fn polygons_are_culled_at_the_share_the_last_frame_culled() {
        let mode = Mode::Position;
        let mut pipelines = Pipelines::default();
        let frame = |pipelines: &mut Pipelines| -> Vec<u32> {
            whole(pipelines, BEGIN_VTXS, mode, 0);
            (0..4)
                .map(|_| {
                    for _ in 0..3 {
                        whole(pipelines, VTX_10, mode, 0);
                    }
                    whole(pipelines, BEGIN_VTXS, mode, 0)
                })
                .collect()
        };
        let built = TRIANGLE - VERTEX_WORK + BEGIN_STALL;
        let culled = CULLED - VERTEX_WORK + BEGIN_STALL;
        assert_eq!(frame(&mut pipelines), [built; 4]);
        pipelines.close_frame(2);
        assert_eq!(frame(&mut pipelines), [built, culled, built, culled]);
        pipelines.close_frame(0);
        assert_eq!(frame(&mut pipelines), [culled; 4]);
    }
}
