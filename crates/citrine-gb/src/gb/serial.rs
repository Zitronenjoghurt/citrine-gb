use crate::gb::ic::{ICInterface, Interrupt};
use std::collections::VecDeque;

pub const DEFAULT_LINK_TIMEOUT_FRAMES: u32 = 300;
const HOLD_CYCLES: u32 = 4096;
const BYTE_DOTS: u32 = 8 * 512;
const FAST_BYTE_DOTS: u32 = 8 * 16;
const FRAME_DOTS: u64 = 70224;
const PENDING_LIMIT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LinkMessage {
    Transfer {
        id: u16,
        byte: u8,
        seq: u16,
        since: u32,
    },
    Reply {
        id: u16,
        byte: u8,
    },
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct MasterTransfer {
    id: u16,
    reply: Option<u8>,
    pulled_at: Option<u8>,
    seq: u16,
    since: u32,
}

impl MasterTransfer {
    fn received(&self) -> Option<u8> {
        let kept = self.pulled_at.unwrap_or(8);
        let ones = 0xFFu8.checked_shr(kept as u32).unwrap_or(0);
        match self.reply {
            Some(reply) => Some((reply & !ones) | ones),
            None if self.pulled_at.is_some() => Some(0xFF),
            None => None,
        }
    }

    fn incoming_bit(&self, index: u8) -> u8 {
        match self.reply {
            Some(reply) if self.pulled_at.is_none_or(|pulled| index < pulled) => {
                (reply >> (7 - index)) & 1
            }
            _ => 1,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct HeldTransfer {
    id: u16,
    byte: u8,
    seq: u16,
    since: u32,
    cycles_left: u32,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct LateByte {
    byte: u8,
    cycles_left: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum First {
    Ours,
    Theirs,
    Both,
}

fn first(ours: (u16, u32, u16), theirs: (u16, u32, u16)) -> First {
    match theirs.0.wrapping_sub(ours.0) as i16 {
        0 => match (ours.1, ours.2).cmp(&(theirs.1, theirs.2)) {
            std::cmp::Ordering::Less => First::Ours,
            std::cmp::Ordering::Greater => First::Theirs,
            std::cmp::Ordering::Equal => First::Both,
        },
        newer if newer > 0 => First::Ours,
        _ => First::Theirs,
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Link {
    plugged: bool,
    outbox: VecDeque<LinkMessage>,
    next_id: u16,
    master: Option<MasterTransfer>,
    held: VecDeque<HeldTransfer>,
    late: VecDeque<LateByte>,
    stalled: u64,
    timeout_frames: u32,
    seq: u16,
    since: u32,
    listening: u64,
    pending: VecDeque<(u16, u16, u32)>,
    armed: Option<(u16, u8)>,
    armed_dots: u32,
    fast: bool,
}

impl Default for Link {
    fn default() -> Self {
        Self {
            plugged: false,
            outbox: VecDeque::new(),
            next_id: 0,
            master: None,
            held: VecDeque::new(),
            late: VecDeque::new(),
            stalled: 0,
            timeout_frames: DEFAULT_LINK_TIMEOUT_FRAMES,
            seq: 0,
            since: 0,
            listening: 0,
            pending: VecDeque::new(),
            armed: None,
            armed_dots: 0,
            fast: false,
        }
    }
}

impl Link {
    fn carry_over(&self) -> Self {
        Self {
            plugged: self.plugged,
            next_id: self.next_id,
            timeout_frames: self.timeout_frames,
            seq: self.seq,
            since: self.since,
            pending: self.pending.clone(),
            ..Self::default()
        }
    }

    fn send(&mut self, message: LinkMessage) {
        if self.plugged {
            self.outbox.push_back(message);
        }
    }

    fn settle(&mut self, seq: u16, since: u32) {
        let anchored = seq == self.seq && seq != 0;
        let rebase = |at: u32| {
            if anchored {
                at.saturating_sub(since)
            } else {
                0
            }
        };
        let next = self.seq.wrapping_add(1);
        if let Some(master) = &mut self.master
            && master.seq == self.seq
        {
            master.seq = next;
            master.since = rebase(master.since);
        }
        for held in self.held.iter_mut().filter(|held| held.seq == self.seq) {
            held.seq = next;
            held.since = rebase(held.since);
        }
        for pending in self
            .pending
            .iter_mut()
            .filter(|pending| pending.1 == self.seq)
        {
            pending.1 = next;
            pending.2 = rebase(pending.2);
        }
        self.seq = next;
        self.since = if anchored {
            self.since.saturating_sub(since)
        } else {
            0
        };
        self.listening = 0;
    }

    fn byte_dots(&self) -> u32 {
        if self.fast { FAST_BYTE_DOTS } else { BYTE_DOTS }
    }

    fn due(&self, held: &HeldTransfer) -> bool {
        held.seq != self.seq || self.seq == 0 || self.since >= held.since
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Serial {
    pub sb: u8,
    sc: u8,
    bits: u8,
    clock_high: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    awaiting: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    sent: Option<u8>,
    #[cfg_attr(feature = "serde", serde(default))]
    link: Link,
}

impl Default for Serial {
    fn default() -> Self {
        Self {
            sb: 0x00,
            sc: 0x02,
            bits: 0,
            clock_high: false,
            awaiting: false,
            sent: None,
            link: Link::default(),
        }
    }
}

impl Serial {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_keeping_link(&mut self) {
        let link = self.link.carry_over();
        *self = Self {
            link,
            ..Self::default()
        };
    }

    pub fn read_sc(&self, cgb_mode: bool) -> u8 {
        if cgb_mode {
            self.sc | 0x7C
        } else {
            self.sc | 0x7E
        }
    }

    pub fn write_sc(&mut self, value: u8, cgb_mode: bool, ic: &mut impl ICInterface) {
        self.bits = 0;
        self.awaiting = false;
        self.link.master = None;
        if self.clock_high {
            self.edge(ic);
        }
        let value = if cgb_mode { value } else { value | 0x02 };
        self.sc = value & 0x83;
        self.link.armed_dots = 0;
        self.link.fast = cgb_mode && self.sc & 0x02 != 0;
        if self.sc & 0x81 == 0x80 {
            self.link.armed = Some((self.link.seq, self.sb));
        }
        if self.sc & 0x81 == 0x81 {
            self.sent = Some(self.sb);
            if self.link.plugged {
                let id = self.link.next_id;
                let (seq, since) = (self.link.seq, self.link.since);
                self.link.next_id = id.wrapping_add(1);
                self.link.listening = 0;
                if self.link.pending.len() == PENDING_LIMIT {
                    self.link.pending.pop_front();
                }
                self.link.pending.push_back((id, seq, since));
                self.link.send(LinkMessage::Transfer {
                    id,
                    byte: self.sb,
                    seq,
                    since,
                });
                self.link.master = Some(MasterTransfer {
                    id,
                    reply: None,
                    pulled_at: None,
                    seq,
                    since,
                });
            }
        }
    }

    pub fn forget_link(&mut self) {
        self.link = Link::default();
    }

    pub fn take_sent(&mut self) -> Option<u8> {
        self.sent.take()
    }

    pub fn transfer_active(&self) -> bool {
        self.sc & 0x80 != 0
    }

    pub fn plugged(&self) -> bool {
        self.link.plugged
    }

    pub fn waiting(&self) -> bool {
        self.awaiting && self.received().is_none()
    }

    pub fn set_timeout_frames(&mut self, frames: u32) {
        self.link.timeout_frames = frames;
    }

    pub fn listening_frames(&self) -> u32 {
        if self.link.plugged {
            (self.link.listening / FRAME_DOTS) as u32
        } else {
            0
        }
    }

    pub fn plug(&mut self) {
        if !self.link.plugged {
            self.link.seq = 0;
            self.link.since = 0;
            self.link.listening = 0;
            self.link.pending.clear();
        }
        self.link.plugged = true;
    }

    pub fn unplug(&mut self, ic: &mut impl ICInterface) {
        self.link.plugged = false;
        self.link.outbox.clear();
        self.link.held.clear();
        self.link.late.clear();
        self.link.pending.clear();
        if let Some(master) = &mut self.link.master
            && master.pulled_at.is_none()
        {
            master.pulled_at = Some(if self.awaiting { 8 } else { self.bits });
        }
        self.resume(ic);
    }

    pub fn take_messages(&mut self) -> Vec<LinkMessage> {
        if self.link.outbox.is_empty() {
            return Vec::new();
        }
        self.link.outbox.drain(..).collect()
    }

    pub fn receive(&mut self, message: LinkMessage, ic: &mut impl ICInterface) {
        if !self.link.plugged {
            return;
        }
        match message {
            LinkMessage::Transfer {
                id,
                byte,
                seq,
                since,
            } => {
                self.link.held.push_back(HeldTransfer {
                    id,
                    byte,
                    seq,
                    since,
                    cycles_left: HOLD_CYCLES,
                });
                self.answer_held(ic);
            }
            LinkMessage::Reply { id, byte } => {
                let (seq, since) = match self.link.pending.iter().position(|p| p.0 == id) {
                    Some(index) => {
                        let (_, seq, since) = self.link.pending.remove(index).unwrap_or_default();
                        (seq, since)
                    }
                    None => (self.link.seq, self.link.since),
                };
                self.link.settle(seq, since);
                if let Some(master) = &mut self.link.master
                    && master.id == id
                    && master.reply.is_none()
                {
                    master.reply = Some(byte);
                    self.resume(ic);
                }
            }
        }
    }

    pub fn stall(&mut self, cycles: u32, frame_cycles: u32, ic: &mut impl ICInterface) -> bool {
        self.resume(ic);
        if !self.awaiting {
            return false;
        }
        self.link.stalled += cycles as u64;
        let limit = self.link.timeout_frames as u64 * frame_cycles as u64;
        if self.link.stalled >= limit {
            self.link.master = None;
            self.complete_master(0xFF, ic);
            return false;
        }
        true
    }

    pub fn tick(&mut self, double_speed: bool, ic: &mut impl ICInterface) {
        if self.link.plugged {
            let dots = if double_speed { 2 } else { 4 };
            self.link.since = self.link.since.saturating_add(dots);
            if self.sc & 0x81 == 0x80 {
                self.link.listening += dots as u64;
                self.link.armed_dots = self.link.armed_dots.saturating_add(dots);
            }
        }
        if self.link.late.is_empty() && self.link.held.is_empty() {
            return;
        }
        if self.sc & 0x81 != 0x81 {
            if let Some(late) = self.link.late.front_mut() {
                late.cycles_left = late.cycles_left.saturating_sub(1);
            }
            let due = self
                .link
                .held
                .front()
                .is_some_and(|held| self.link.due(held));
            if due && let Some(held) = self.link.held.front_mut() {
                held.cycles_left = held.cycles_left.saturating_sub(1);
            }
        }
        self.answer_held(ic);
    }

    #[inline]
    pub fn div_changed(&mut self, old: u16, new: u16, cgb_mode: bool, ic: &mut impl ICInterface) {
        let mask = if cgb_mode && self.sc & 0x02 != 0 {
            0x04
        } else {
            0x80
        };
        if old & !new & mask != 0 {
            self.edge(ic);
        }
    }

    fn answer_held(&mut self, ic: &mut impl ICInterface) {
        let listening = self.sc & 0x81 == 0x80 && self.link.armed_dots >= self.link.byte_dots();
        if let Some(late) = self.link.late.front().copied() {
            match self.sc & 0x81 {
                0x80 if listening => {
                    self.take_byte(late.byte, ic);
                    self.link.late.pop_front();
                }
                0x80 | 0x81 => {}
                _ if late.cycles_left == 0 => {
                    self.link.late.pop_front();
                }
                _ => {}
            }
        }
        let Some(held) = self.link.held.front().copied() else {
            return;
        };
        let due = self.link.due(&held);
        let reply = match self.sc & 0x81 {
            0x81 => match self.link.master.map(|master| {
                first(
                    (master.seq, master.since, master.id),
                    (held.seq, held.since, held.id),
                )
            }) {
                Some(First::Ours) => return,
                Some(First::Theirs) => {
                    self.link.late.push_back(LateByte {
                        byte: held.byte,
                        cycles_left: HOLD_CYCLES,
                    });
                    match self.link.armed {
                        Some((seq, sb)) if seq == held.seq => sb,
                        _ => 0x00,
                    }
                }
                _ => 0xFF,
            },
            0x80 if listening => self.take_byte(held.byte, ic),
            _ if due && held.cycles_left == 0 => 0x00,
            _ => return,
        };
        self.link.held.pop_front();
        self.link.settle(held.seq, held.since);
        self.link.send(LinkMessage::Reply {
            id: held.id,
            byte: reply,
        });
    }

    fn take_byte(&mut self, byte: u8, ic: &mut impl ICInterface) -> u8 {
        let reply = self.sb;
        self.sb = byte;
        self.bits = 0;
        self.sc &= !0x80;
        ic.request_interrupt(Interrupt::Serial);
        reply
    }

    fn resume(&mut self, ic: &mut impl ICInterface) {
        if !self.awaiting {
            return;
        }
        if let Some(byte) = self.received() {
            self.link.master = None;
            self.complete_master(byte, ic);
        }
    }

    fn received(&self) -> Option<u8> {
        match &self.link.master {
            Some(master) => master.received(),
            None => Some(0xFF),
        }
    }

    fn complete_master(&mut self, byte: u8, ic: &mut impl ICInterface) {
        self.awaiting = false;
        self.link.stalled = 0;
        self.sb = byte;
        self.sc &= !0x80;
        ic.request_interrupt(Interrupt::Serial);
    }

    fn edge(&mut self, ic: &mut impl ICInterface) {
        self.clock_high = !self.clock_high;
        if self.clock_high || self.sc & 0x81 != 0x81 || self.awaiting {
            return;
        }

        let bit = self
            .link
            .master
            .map_or(1, |master| master.incoming_bit(self.bits));
        self.bits += 1;
        self.sb = (self.sb << 1) | bit;
        if self.bits == 8 {
            self.bits = 0;
            self.awaiting = true;
            self.resume(ic);
        }
    }
}
