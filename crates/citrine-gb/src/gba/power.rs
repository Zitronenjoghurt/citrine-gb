const STOP: u8 = 1 << 7;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Power {
    #[default]
    Running,
    Halting,
    Halted,
    Stopped,
}

impl Power {
    pub fn from_haltcnt(value: u8) -> Self {
        if value & STOP == 0 {
            Self::Halting
        } else {
            Self::Stopped
        }
    }

    pub fn running(self) -> bool {
        self == Self::Running
    }

    pub fn settled(self) -> Self {
        match self {
            Self::Halting => Self::Halted,
            power => power,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haltcnt_bit_7_picks_stop_over_halt() {
        assert_eq!(Power::from_haltcnt(0x00), Power::Halting);
        assert_eq!(Power::from_haltcnt(0x7F), Power::Halting);
        assert_eq!(Power::from_haltcnt(0x80), Power::Stopped);
    }

    #[test]
    fn a_halt_write_settles_into_halted() {
        assert_eq!(Power::Halting.settled(), Power::Halted);
        assert_eq!(Power::Running.settled(), Power::Running);
        assert_eq!(Power::Stopped.settled(), Power::Stopped);
    }
}
