use super::frame::Frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Slot {
    Loc1,
    Cmd,
    Loc2,
    Loc3,
    Beacon,
    Reply,
    EmptyReply,
    Ack,
}

impl Slot {
    pub const QUEUED: [Slot; 5] = [Slot::Beacon, Slot::Loc3, Slot::Loc2, Slot::Cmd, Slot::Loc1];

    pub fn busy_bit(self) -> u16 {
        match self {
            Slot::Loc1 => 1 << 0,
            Slot::Cmd | Slot::Ack => 1 << 1,
            Slot::Loc2 => 1 << 2,
            Slot::Loc3 => 1 << 3,
            Slot::Beacon => 1 << 4,
            Slot::Reply | Slot::EmptyReply => 0,
        }
    }

    pub fn request_bit(self) -> u16 {
        match self {
            Slot::Beacon => 0,
            other => other.busy_bit(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sending {
    pub slot: Slot,
    pub header: Option<usize>,
    pub frame: Frame,
    pub started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Purpose {
    Normal,
    HostReply(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Receiving {
    pub frame: Frame,
    pub flags: u16,
    pub purpose: Purpose,
    pub started: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Exchange {
    pub header: usize,
    pub clients: u16,
    pub pending: u16,
    pub reply_time: u64,
    pub cmd_end: u64,
    pub window_end: u64,
    pub rounds: u8,
}

impl Exchange {
    pub fn slot_start(&self, slot: u32) -> u64 {
        self.cmd_end + super::mp::FIRST_REPLY_GAP_US + u64::from(slot) * self.slot_us()
    }

    pub fn slot_us(&self) -> u64 {
        super::mp::SLOT_GAP_US + self.reply_time
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PendingReply {
    pub frame: Frame,
    pub header: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Link {
    #[default]
    Idle,
    Sending(Sending),
    Receiving(Receiving),
    Listening,
    Waiting(u64, PendingReply),
    Failing,
}

impl Link {
    pub fn in_flight(&self) -> bool {
        matches!(self, Link::Sending(_) | Link::Receiving(_))
    }

    pub fn deadline(&self) -> Option<u64> {
        match self {
            Link::Idle | Link::Listening | Link::Failing => None,
            Link::Sending(sending) => Some(if sending.started {
                sending.frame.end()
            } else {
                sending.frame.start + sending.frame.preamble_us()
            }),
            Link::Receiving(receiving) => Some(if receiving.started {
                receiving.frame.end()
            } else {
                receiving.frame.start + receiving.frame.preamble_us()
            }),
            Link::Waiting(until, _) => Some(*until),
        }
    }
}
