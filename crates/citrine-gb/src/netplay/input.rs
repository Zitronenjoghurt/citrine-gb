use crate::console::Keys;

pub(crate) const LOG_LEN: usize = 512;

pub const MAX_UNITS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Player(u8);

#[expect(
    non_upper_case_globals,
    reason = "players read as Player::One to Player::Four"
)]
impl Player {
    pub const One: Self = Self(0);
    pub const Two: Self = Self(1);
    pub const Three: Self = Self(2);
    pub const Four: Self = Self(3);
    pub const ALL: [Self; MAX_UNITS] = [Self::One, Self::Two, Self::Three, Self::Four];

    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    pub fn other(self) -> Self {
        Self(self.0 ^ 1)
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
    pub buttons: Keys,
    pub cable: Option<CableEvent>,
}

impl Input {
    pub const fn new(buttons: Keys) -> Self {
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
    pub(crate) fn starting_at(frame: u32) -> Self {
        Self {
            entries: vec![None; LOG_LEN].into_boxed_slice(),
            frontier: frame,
            latest: frame.checked_sub(1),
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
