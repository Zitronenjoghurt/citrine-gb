pub const FLAG_N: u32 = 1 << 31;
pub const FLAG_Z: u32 = 1 << 30;
pub const FLAG_C: u32 = 1 << 29;
pub const FLAG_V: u32 = 1 << 28;
pub const FLAG_I: u32 = 1 << 7;
pub const FLAG_F: u32 = 1 << 6;
pub const FLAG_T: u32 = 1 << 5;
pub const MODE_MASK: u32 = 0x1F;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    User,
    Fiq,
    Irq,
    Supervisor,
    Abort,
    Undefined,
    System,
}

impl Mode {
    pub fn from_bits(bits: u32) -> Option<Self> {
        match bits & MODE_MASK {
            0x10 => Some(Self::User),
            0x11 => Some(Self::Fiq),
            0x12 => Some(Self::Irq),
            0x13 => Some(Self::Supervisor),
            0x17 => Some(Self::Abort),
            0x1B => Some(Self::Undefined),
            0x1F => Some(Self::System),
            _ => None,
        }
    }

    pub fn bits(self) -> u32 {
        match self {
            Self::User => 0x10,
            Self::Fiq => 0x11,
            Self::Irq => 0x12,
            Self::Supervisor => 0x13,
            Self::Abort => 0x17,
            Self::Undefined => 0x1B,
            Self::System => 0x1F,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Bank {
    User,
    Fiq,
    Supervisor,
    Abort,
    Irq,
    Undefined,
}

impl Bank {
    pub fn of(cpsr: u32) -> Self {
        match Mode::from_bits(cpsr) {
            Some(Mode::Fiq) => Self::Fiq,
            Some(Mode::Irq) => Self::Irq,
            Some(Mode::Supervisor) => Self::Supervisor,
            Some(Mode::Abort) => Self::Abort,
            Some(Mode::Undefined) => Self::Undefined,
            Some(Mode::User | Mode::System) | None => Self::User,
        }
    }

    pub fn spsr_index(self) -> Option<usize> {
        match self {
            Self::User => None,
            Self::Fiq => Some(0),
            Self::Supervisor => Some(1),
            Self::Abort => Some(2),
            Self::Irq => Some(3),
            Self::Undefined => Some(4),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Psr(pub u32);

impl Psr {
    pub fn n(self) -> bool {
        self.0 & FLAG_N != 0
    }

    pub fn z(self) -> bool {
        self.0 & FLAG_Z != 0
    }

    pub fn c(self) -> bool {
        self.0 & FLAG_C != 0
    }

    pub fn v(self) -> bool {
        self.0 & FLAG_V != 0
    }

    pub fn passes(self, condition: u32) -> bool {
        match condition & 0xF {
            0x0 => self.z(),
            0x1 => !self.z(),
            0x2 => self.c(),
            0x3 => !self.c(),
            0x4 => self.n(),
            0x5 => !self.n(),
            0x6 => self.v(),
            0x7 => !self.v(),
            0x8 => self.c() && !self.z(),
            0x9 => !self.c() || self.z(),
            0xA => self.n() == self.v(),
            0xB => self.n() != self.v(),
            0xC => !self.z() && self.n() == self.v(),
            0xD => self.z() || self.n() != self.v(),
            0xE => true,
            _ => false,
        }
    }
}
