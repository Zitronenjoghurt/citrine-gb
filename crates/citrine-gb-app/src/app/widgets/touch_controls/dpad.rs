use citrine_gb::gb::joypad::JoypadState;
use egui::Vec2;

pub const DEAD_ZONE: f32 = 0.2;
pub const HOLD_RING: f32 = 1.45;
const ARM_HALF: f32 = 30.0;
const DIAGONAL_HALF: f32 = 15.0;
const BAND: f32 = 6.0;

pub const ALL: JoypadState = JoypadState::RIGHT
    .union(JoypadState::DOWN)
    .union(JoypadState::LEFT)
    .union(JoypadState::UP);

const SLICES: [(f32, f32, JoypadState); 8] = [
    (0.0, ARM_HALF, JoypadState::RIGHT),
    (90.0, ARM_HALF, JoypadState::DOWN),
    (180.0, ARM_HALF, JoypadState::LEFT),
    (270.0, ARM_HALF, JoypadState::UP),
    (
        45.0,
        DIAGONAL_HALF,
        JoypadState::RIGHT.union(JoypadState::DOWN),
    ),
    (
        135.0,
        DIAGONAL_HALF,
        JoypadState::DOWN.union(JoypadState::LEFT),
    ),
    (
        225.0,
        DIAGONAL_HALF,
        JoypadState::LEFT.union(JoypadState::UP),
    ),
    (
        315.0,
        DIAGONAL_HALF,
        JoypadState::UP.union(JoypadState::RIGHT),
    ),
];

pub fn direction(offset: Vec2, radius: f32, held: JoypadState) -> JoypadState {
    if radius <= 0.0 {
        return JoypadState::empty();
    }
    let reach = offset.length() / radius;
    if !(DEAD_ZONE..=HOLD_RING).contains(&reach) {
        return JoypadState::empty();
    }

    let angle = offset.y.atan2(offset.x).to_degrees();
    let held = held & ALL;
    let kept = SLICES
        .iter()
        .any(|&(centre, half, state)| state == held && apart(angle, centre) <= half + BAND);
    if kept {
        return held;
    }

    SLICES
        .iter()
        .find(|&&(centre, half, _)| apart(angle, centre) <= half + 1e-3)
        .map_or(JoypadState::empty(), |&(_, _, state)| state)
}

fn apart(angle: f32, centre: f32) -> f32 {
    ((angle - centre + 180.0).rem_euclid(360.0) - 180.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RADIUS: f32 = 86.0;

    fn at(degrees: f32, reach: f32) -> Vec2 {
        let r = degrees.to_radians();
        Vec2::new(r.cos(), r.sin()) * RADIUS * reach
    }

    fn read(degrees: f32, held: JoypadState) -> JoypadState {
        direction(at(degrees, 0.6), RADIUS, held)
    }

    #[track_caller]
    fn assert_reads(actual: JoypadState, expected: JoypadState, context: &str) {
        assert_eq!(
            actual.bits(),
            expected.bits(),
            "{context}: expected {expected:?}, got {actual:?}"
        );
    }

    const RIGHT_DOWN: JoypadState = JoypadState::RIGHT.union(JoypadState::DOWN);
    const RIGHT_UP: JoypadState = JoypadState::RIGHT.union(JoypadState::UP);

    #[test]
    fn up_to_thirty_degrees_off_an_arm_reads_that_arm() {
        for off in [0.0, 22.0, 25.0, 27.0, 28.0, 30.0] {
            assert_reads(
                read(off, JoypadState::empty()),
                JoypadState::RIGHT,
                &format!("{off} degrees below Right"),
            );
            assert_reads(
                read(-off, JoypadState::empty()),
                JoypadState::RIGHT,
                &format!("{off} degrees above Right"),
            );
        }
    }

    #[test]
    fn every_arm_is_sixty_degrees_wide() {
        for (centre, arm) in [
            (0.0, JoypadState::RIGHT),
            (90.0, JoypadState::DOWN),
            (180.0, JoypadState::LEFT),
            (270.0, JoypadState::UP),
        ] {
            for off in [-27.0, 0.0, 27.0] {
                assert_reads(
                    read(centre + off, JoypadState::empty()),
                    arm,
                    &format!("{off} degrees off {arm:?}"),
                );
            }
        }
    }

    #[test]
    fn a_diagonal_is_thirty_degrees_wide() {
        assert_reads(read(45.0, JoypadState::empty()), RIGHT_DOWN, "45 degrees");
        assert_reads(read(31.0, JoypadState::empty()), RIGHT_DOWN, "31 degrees");
        assert_reads(read(59.0, JoypadState::empty()), RIGHT_DOWN, "59 degrees");
        assert_reads(read(-45.0, JoypadState::empty()), RIGHT_UP, "45 above");
        assert_reads(
            read(225.0, JoypadState::empty()),
            JoypadState::LEFT | JoypadState::UP,
            "up-left",
        );
        assert_reads(
            read(135.0, JoypadState::empty()),
            JoypadState::DOWN | JoypadState::LEFT,
            "down-left",
        );
    }

    #[test]
    fn a_held_arm_keeps_six_degrees_past_its_edge() {
        assert_reads(read(33.0, JoypadState::RIGHT), JoypadState::RIGHT, "33");
        assert_reads(read(35.9, JoypadState::RIGHT), JoypadState::RIGHT, "35.9");
        assert_reads(read(36.5, JoypadState::RIGHT), RIGHT_DOWN, "36.5");
    }

    #[test]
    fn a_held_diagonal_keeps_six_degrees_past_its_edge() {
        assert_reads(read(26.0, RIGHT_DOWN), RIGHT_DOWN, "26");
        assert_reads(read(24.5, RIGHT_DOWN), RIGHT_DOWN, "24.5");
        assert_reads(read(23.0, RIGHT_DOWN), JoypadState::RIGHT, "23");
    }

    #[test]
    fn a_contact_wobbling_on_a_slice_line_does_not_flip() {
        let mut held = read(28.0, JoypadState::empty());
        let mut changes = 0;
        for off in [29.0, 31.0, 33.0, 30.0, 34.0, 31.5, 28.0, 35.0, 32.0] {
            let next = read(off, held);
            if next != held {
                changes += 1;
            }
            held = next;
        }
        assert_eq!(changes, 0, "the band absorbs a 7 degree wobble");
        assert_reads(held, JoypadState::RIGHT, "still Right");
    }

    #[test]
    fn a_slide_from_right_to_down_changes_twice() {
        let mut held = JoypadState::empty();
        let mut seen = Vec::new();
        for step in 0..=90 {
            let next = read(step as f32, held);
            if next != held {
                seen.push(next);
            }
            held = next;
        }
        assert_eq!(
            seen,
            vec![JoypadState::RIGHT, RIGHT_DOWN, JoypadState::DOWN],
            "Right, then Right+Down, then Down, each once"
        );
    }

    #[test]
    fn the_middle_presses_nothing() {
        assert_reads(
            direction(at(0.0, 0.19), RADIUS, JoypadState::empty()),
            JoypadState::empty(),
            "inside the dead zone",
        );
        assert_reads(
            direction(at(0.0, 0.21), RADIUS, JoypadState::empty()),
            JoypadState::RIGHT,
            "just outside it",
        );
    }

    #[test]
    fn a_finger_past_the_drawing_keeps_steering_up_to_the_hold_ring() {
        assert_reads(
            direction(at(10.0, 1.2), RADIUS, JoypadState::RIGHT),
            JoypadState::RIGHT,
            "past the arm's tip",
        );
        assert_reads(
            direction(at(10.0, 1.44), RADIUS, JoypadState::RIGHT),
            JoypadState::RIGHT,
            "just inside the ring",
        );
        assert_reads(
            direction(at(10.0, 1.46), RADIUS, JoypadState::RIGHT),
            JoypadState::empty(),
            "outside the ring",
        );
    }

    #[test]
    fn other_buttons_held_do_not_confuse_the_band() {
        assert_reads(
            read(33.0, JoypadState::RIGHT | JoypadState::A),
            JoypadState::RIGHT,
            "A held by another finger",
        );
    }
}
