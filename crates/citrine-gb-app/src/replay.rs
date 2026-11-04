use citrine_gb::console::Keys;
use citrine_gb::recording::{InputEvent, Recording};

pub const T_CYCLES_PER_SECOND: u64 = 4_194_304;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error("this isn't an input replay")]
    Unreadable,
    #[error("load {0} first")]
    OtherGame(String),
}

pub struct ReplayPlayer {
    events: Vec<InputEvent>,
    next: usize,
    held: Keys,
}

pub fn parse(json: &[u8]) -> Result<Recording, ReplayError> {
    let text = std::str::from_utf8(json).map_err(|_| ReplayError::Unreadable)?;
    let mut recording = Recording::from_json(text).map_err(|_| ReplayError::Unreadable)?;
    recording.sort();
    Ok(recording)
}

pub fn length_ms(recording: &Recording) -> u64 {
    let end = recording
        .events
        .last()
        .map_or(0, |e| e.cycle)
        .max(recording.end_cycle);
    end.saturating_mul(1000) / T_CYCLES_PER_SECOND
}

impl ReplayPlayer {
    pub fn new(recording: Recording) -> Self {
        Self {
            events: recording.events,
            next: 0,
            held: Keys::empty(),
        }
    }

    pub fn due(&mut self, t_cycle: u64) -> Vec<(Keys, bool)> {
        let mut out = Vec::new();
        while let Some(event) = self.events.get(self.next) {
            if event.cycle > t_cycle {
                break;
            }
            let button = event.button.to_keys();
            if event.pressed {
                self.held.insert(button);
            } else {
                self.held.remove(button);
            }
            out.push((button, event.pressed));
            self.next += 1;
        }
        out
    }

    pub fn finished(&self) -> bool {
        self.next >= self.events.len()
    }

    pub fn held(&self) -> Keys {
        self.held
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.next, self.events.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::gb::GbModel;
    use citrine_gb::recording::Button;

    fn recording() -> Recording {
        let mut r = Recording::new("ab".repeat(32), GbModel::Dmg);
        r.push(100, Button::A, true);
        r.push(T_CYCLES_PER_SECOND, Button::A, false);
        r.push(50, Button::Start, true);
        r
    }

    #[test]
    fn events_come_due_in_cycle_order() {
        let json = recording().to_json().unwrap();
        let parsed = parse(json.as_bytes()).unwrap();
        assert_eq!(length_ms(&parsed), 1000);
        let mut player = ReplayPlayer::new(parsed);
        assert!(player.due(10).is_empty());
        assert_eq!(player.due(100), vec![(Keys::START, true), (Keys::A, true)]);
        assert_eq!(player.held(), Keys::START | Keys::A);
        assert!(!player.finished());
        assert_eq!(player.due(u64::MAX), vec![(Keys::A, false)]);
        assert!(player.finished());
        assert_eq!(player.progress(), (3, 3));
    }

    #[test]
    fn junk_is_not_a_replay() {
        assert_eq!(parse(b"{}").err(), Some(ReplayError::Unreadable));
        assert_eq!(parse(&[0xff, 0xfe]).err(), Some(ReplayError::Unreadable));
    }
}
