use std::collections::VecDeque;

const SMOOTHING: f32 = 0.002;
const MAX_QUEUED: usize = 8_192;

pub fn level(volume: f32, muted: bool) -> f32 {
    if muted {
        0.0
    } else {
        let volume = volume.clamp(0.0, 1.0);
        volume * volume
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundLevels {
    pub mine: f32,
    pub friend: f32,
}

#[derive(Default)]
pub struct Mixer {
    applied: Option<SoundLevels>,
    queued: VecDeque<f32>,
}

impl Mixer {
    pub fn mix(&mut self, target: SoundLevels, mine: &mut Vec<f32>, friend: &mut Vec<f32>) {
        let mut applied = self.applied.unwrap_or(target);
        if mine.len() < friend.len() {
            mine.resize(friend.len(), 0.0);
        }
        if target.friend == 0.0 && applied.friend == 0.0 {
            self.queued.clear();
            friend.clear();
        } else {
            self.queued.extend(friend.drain(..));
        }
        for frame in mine.chunks_mut(2) {
            applied.mine += (target.mine - applied.mine) * SMOOTHING;
            applied.friend += (target.friend - applied.friend) * SMOOTHING;
            for sample in frame {
                let theirs = self.queued.pop_front().unwrap_or(0.0);
                *sample = (*sample * applied.mine + theirs * applied.friend).clamp(-1.0, 1.0);
            }
        }
        while self.queued.len() > MAX_QUEUED {
            self.queued.pop_front();
            self.queued.pop_front();
        }
        self.applied = Some(applied);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixed(levels: SoundLevels, mine: &[f32], friend: &[f32]) -> Vec<f32> {
        let mut out = mine.to_vec();
        Mixer::default().mix(levels, &mut out, &mut friend.to_vec());
        out
    }

    fn assert_near(got: Vec<f32>, want: &[f32]) {
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-6, "got {got:?}, want {want:?}");
        }
    }

    #[test]
    fn the_friends_sound_adds_to_mine_at_its_own_level_and_clamps() {
        let levels = SoundLevels {
            mine: 1.0,
            friend: level(0.5, false),
        };
        let out = mixed(levels, &[0.2, -0.2, 0.9, -0.9], &[0.4, 0.4, 0.8, -0.8]);
        assert_near(out, &[0.3, -0.1, 1.0, -1.0]);
    }

    #[test]
    fn a_muted_friend_leaves_only_my_sound() {
        let levels = SoundLevels {
            mine: level(0.8, false),
            friend: level(0.5, true),
        };
        let out = mixed(levels, &[0.5, -0.5], &[0.4, 0.4]);
        assert_near(out, &[0.5 * 0.64, -0.5 * 0.64]);
    }

    #[test]
    fn muting_my_game_still_plays_the_friend_at_their_level() {
        let levels = SoundLevels {
            mine: level(0.8, true),
            friend: level(0.5, false),
        };
        let out = mixed(levels, &[0.5, -0.5], &[0.4, -0.8]);
        assert_near(out, &[0.1, -0.2]);
    }

    #[test]
    fn no_friend_samples_leave_only_my_sound() {
        let levels = SoundLevels {
            mine: 1.0,
            friend: level(0.5, false),
        };
        let out = mixed(levels, &[0.25, -0.25], &[]);
        assert_near(out, &[0.25, -0.25]);
    }

    #[test]
    fn the_friend_plays_while_my_console_makes_no_samples() {
        let levels = SoundLevels {
            mine: 1.0,
            friend: level(0.5, false),
        };
        let out = mixed(levels, &[], &[0.4, -0.4]);
        assert_near(out, &[0.1, -0.1]);
    }

    #[test]
    fn a_level_change_glides_instead_of_jumping() {
        let mut mixer = Mixer::default();
        let loud = SoundLevels {
            mine: 1.0,
            friend: 0.0,
        };
        mixer.mix(loud, &mut vec![0.5, 0.5], &mut Vec::new());
        let mut out = vec![0.5; 4_000];
        let quiet = SoundLevels { mine: 0.0, ..loud };
        mixer.mix(quiet, &mut out, &mut Vec::new());
        assert!(out[0] > 0.49);
        assert!(out[out.len() - 1] < 0.05);
        assert!(out.windows(2).all(|w| w[1] <= w[0]));
    }
}
