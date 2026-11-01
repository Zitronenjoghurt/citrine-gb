use crate::gb::joypad::JoypadState;

pub(crate) const LOG_LEN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Player {
    One,
    Two,
}

impl Player {
    pub fn index(self) -> usize {
        match self {
            Player::One => 0,
            Player::Two => 1,
        }
    }

    pub fn other(self) -> Self {
        match self {
            Player::One => Player::Two,
            Player::Two => Player::One,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CableEvent {
    Plug,
    Pull,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Input {
    pub buttons: JoypadState,
    pub cable: Option<CableEvent>,
}

impl Input {
    pub const fn new(buttons: JoypadState) -> Self {
        Self {
            buttons,
            cable: None,
        }
    }

    pub fn with_cable(self, event: CableEvent) -> Self {
        Self {
            cable: Some(event),
            ..self
        }
    }

    fn held(self) -> Self {
        Self::new(self.buttons)
    }
}

pub(crate) struct InputLog {
    entries: Box<[Option<(u32, Input)>]>,
    frontier: u32,
    latest: Option<u32>,
}

impl InputLog {
    pub(crate) fn new() -> Self {
        Self {
            entries: vec![None; LOG_LEN].into_boxed_slice(),
            frontier: 0,
            latest: None,
        }
    }

    pub(crate) fn get(&self, frame: u32) -> Option<Input> {
        match self.entries[frame as usize % LOG_LEN] {
            Some((at, input)) if at == frame => Some(input),
            _ => None,
        }
    }

    pub(crate) fn set(&mut self, frame: u32, input: Input) {
        self.entries[frame as usize % LOG_LEN] = Some((frame, input));
        self.latest = Some(self.latest.map_or(frame, |latest| latest.max(frame)));
        while self.get(self.frontier).is_some() {
            self.frontier += 1;
        }
    }

    pub(crate) fn frontier(&self) -> u32 {
        self.frontier
    }

    pub(crate) fn latest(&self) -> Option<u32> {
        self.latest
    }

    pub(crate) fn accepts(&self, frame: u32, ahead: u32) -> bool {
        frame >= self.frontier && frame < self.frontier.saturating_add(ahead)
    }

    pub(crate) fn predict(&self, frame: u32) -> Input {
        let floor = self.frontier.saturating_sub(1);
        (floor..frame)
            .rev()
            .find_map(|at| self.get(at))
            .map(Input::held)
            .unwrap_or_default()
    }
}
