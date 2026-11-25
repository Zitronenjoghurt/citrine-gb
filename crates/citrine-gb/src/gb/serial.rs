#[cfg(feature = "persistence")]
use crate::gb::GameBoy;
use crate::gb::ic::{ICInterface, Interrupt};
use crate::gb::printer::{PrintedImage, Printer};
#[cfg(feature = "persistence")]
use crate::netplay::LinkPort;
use crate::traffic::{Entry, Kind, Traffic};
use std::collections::VecDeque;

pub const DEFAULT_LINK_TIMEOUT_FRAMES: u32 = 300;
const HOLD_CYCLES: u32 = 4096;
const BYTE_DOTS: u32 = 8 * 512;
const FAST_BYTE_DOTS: u32 = 8 * 16;
const FRAME_DOTS: u64 = 70224;
const PENDING_LIMIT: usize = 16;
const HELD_LIMIT: usize = 16;

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
    #[cfg_attr(feature = "serde", serde(default))]
    printer: Option<Printer>,
    #[cfg_attr(feature = "serde", serde(skip))]
    outgoing: u8,
    #[cfg_attr(feature = "serde", serde(skip))]
    traffic: Traffic,
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
            printer: None,
            outgoing: 0,
            traffic: Traffic::default(),
        }
    }
}

impl Serial {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_keeping_link(&mut self) {
        let link = self.link.carry_over();
        let printer = self.printer.as_ref().map(|_| Printer::default());
        let mut traffic = std::mem::take(&mut self.traffic);
        traffic.push(Entry::new(Kind::SoftReset));
        *self = Self {
            link,
            printer,
            traffic,
            ..Self::default()
        };
    }

    pub fn copy_keeping_traffic(&mut self, source: &Serial) {
        let traffic = std::mem::take(&mut self.traffic);
        self.clone_from(source);
        self.traffic = traffic;
    }

    pub fn traffic(&self) -> &Traffic {
        &self.traffic
    }

    pub fn next_traffic_frame(&mut self) {
        self.traffic.next_frame();
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
        self.outgoing = self.sb;
        self.traffic
            .push(Entry::new(Kind::SerialControl).sent(&[self.sc, self.sb]));
        self.link.armed_dots = 0;
        self.link.fast = cgb_mode && self.sc & 0x02 != 0;
        if self.sc & 0x81 == 0x80 {
            self.link.armed = Some((self.link.seq, self.sb));
        }
        if self.sc & 0x81 == 0x81 {
            self.sent = Some(self.sb);
            if !self.link.plugged
                && let Some(printer) = &mut self.printer
            {
                self.link.master = Some(MasterTransfer {
                    id: 0,
                    reply: Some(printer.exchange(self.sb)),
                    pulled_at: None,
                    seq: 0,
                    since: 0,
                });
            } else if self.link.plugged {
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

    pub fn attach_printer(&mut self, attached: bool) {
        self.printer = attached.then(Printer::default);
    }

    pub fn printer_attached(&self) -> bool {
        self.printer.is_some()
    }

    pub fn take_prints(&mut self) -> Vec<PrintedImage> {
        self.printer
            .as_mut()
            .map(Printer::take_prints)
            .unwrap_or_default()
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

    pub fn awaiting(&self) -> bool {
        self.awaiting
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

    pub fn link_traffic_idle(&self) -> bool {
        self.link.plugged
            && self.link.late.is_empty()
            && self.link.held.is_empty()
            && self.link.outbox.is_empty()
    }

    pub fn advance_idle(&mut self, cycles: u32, double_speed: bool) {
        if !self.link.plugged {
            return;
        }
        let dots = cycles * if double_speed { 2 } else { 4 };
        self.link.since = self.link.since.saturating_add(dots);
        if self.sc & 0x81 == 0x80 {
            self.link.listening += dots as u64;
            self.link.armed_dots = self.link.armed_dots.saturating_add(dots);
        }
    }

    pub fn link_silent(&self) -> bool {
        self.link_traffic_idle() && !self.awaiting && self.sc & 0x81 != 0x81
    }

    #[inline]
    pub fn take_messages(&mut self) -> Vec<LinkMessage> {
        if self.link.outbox.is_empty() {
            return Vec::new();
        }
        self.drain_outbox()
    }

    #[inline(never)]
    fn drain_outbox(&mut self) -> Vec<LinkMessage> {
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
                if self.link.held.len() == HELD_LIMIT {
                    self.link.held.pop_front();
                }
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

    #[inline]
    pub fn stall(&mut self, cycles: u32, frame_cycles: u32, ic: &mut impl ICInterface) -> bool {
        self.awaiting && self.stall_awaiting(cycles, frame_cycles, ic)
    }

    #[inline(never)]
    fn stall_awaiting(
        &mut self,
        cycles: u32,
        frame_cycles: u32,
        ic: &mut impl ICInterface,
    ) -> bool {
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

    #[inline]
    pub fn tick(&mut self, double_speed: bool, ic: &mut impl ICInterface) {
        if !self.link.plugged {
            debug_assert!(self.link.late.is_empty() && self.link.held.is_empty());
            return;
        }
        self.tick_plugged(double_speed, ic);
    }

    #[inline(always)]
    pub(crate) fn tick_plugged(&mut self, double_speed: bool, ic: &mut impl ICInterface) {
        debug_assert!(self.link.plugged);
        let dots = if double_speed { 2 } else { 4 };
        self.link.since = self.link.since.saturating_add(dots);
        if self.sc & 0x81 == 0x80 {
            self.link.listening += dots as u64;
            self.link.armed_dots = self.link.armed_dots.saturating_add(dots);
        }
        if self.link.late.is_empty() && self.link.held.is_empty() {
            return;
        }
        self.tick_queued(ic);
    }

    #[inline(never)]
    fn tick_queued(&mut self, ic: &mut impl ICInterface) {
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

    pub fn quiet(&self) -> bool {
        !self.link.plugged
            && !self.awaiting
            && self.link.late.is_empty()
            && self.link.held.is_empty()
            && self.sc & 0x81 != 0x81
    }

    fn clock_period(&self, cgb_mode: bool) -> u32 {
        if cgb_mode && self.sc & 0x02 != 0 {
            8
        } else {
            256
        }
    }

    pub fn edge_free_cycles(&self, div: u16, cgb_mode: bool) -> u32 {
        if self.sc & 0x81 != 0x81 {
            return u32::MAX;
        }
        let period = self.clock_period(cgb_mode);
        let div = u32::from(div);
        ((div / period + 1) * period - 1 - div) / 4
    }

    pub fn div_advanced(&mut self, old: u16, cycles: u32, cgb_mode: bool) {
        let period = self.clock_period(cgb_mode);
        let old = u32::from(old);
        let edges = (old + cycles * 4) / period - old / period;
        if edges & 1 != 0 {
            self.clock_high = !self.clock_high;
        }
    }

    #[inline]
    pub fn div_changed(&mut self, old: u16, new: u16, cgb_mode: bool, ic: &mut impl ICInterface) {
        let fell = old & !new;
        if fell == 0 {
            return;
        }
        let mask = if cgb_mode && self.sc & 0x02 != 0 {
            0x04
        } else {
            0x80
        };
        if fell & mask != 0 {
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
                    if self.link.late.len() == HELD_LIMIT {
                        self.link.late.pop_front();
                    }
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
        self.traffic
            .push(Entry::new(Kind::SerialDone).sent(&[reply]).got(&[byte]));
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
        self.traffic.push(
            Entry::new(Kind::SerialDone)
                .tag(1)
                .sent(&[self.outgoing])
                .got(&[byte]),
        );
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

#[cfg(feature = "persistence")]
impl LinkPort for GameBoy {
    type Message = LinkMessage;

    fn plug_link(&mut self) {
        GameBoy::plug_link(self);
    }

    fn unplug_link(&mut self) {
        GameBoy::unplug_link(self);
    }

    fn link_plugged(&self) -> bool {
        GameBoy::link_plugged(self)
    }

    fn link_waiting(&self) -> bool {
        GameBoy::link_waiting(self)
    }

    #[inline]
    fn take_link_messages(&mut self) -> Vec<LinkMessage> {
        GameBoy::take_link_messages(self)
    }

    fn receive_link_message(&mut self, message: LinkMessage) {
        GameBoy::receive_link_message(self, message);
    }

    fn is_transfer(message: &LinkMessage) -> bool {
        matches!(message, LinkMessage::Transfer { .. })
    }

    fn begin_linked_frame(&mut self) -> u64 {
        self.ppu.frame_ready = false;
        0
    }

    fn step_linked(&mut self) -> u64 {
        let before = self.cycle_counter;
        self.step_unsettled(1);
        let spent = self.cycle_counter.wrapping_sub(before) as u64;
        if self.cgb.double_speed {
            spent
        } else {
            spent * 2
        }
    }

    fn step_linked_within(&mut self, room: u64) -> u64 {
        let before = self.cycle_counter;
        let mut budget = 1;
        if self.takes_room() {
            budget = 1 + (room.saturating_sub(1) >> u32::from(!self.cgb.double_speed));
            if !self.ppu.lcdc.lcd_enabled {
                let left = self.frame_cycles().saturating_sub(self.cycle_counter);
                budget = budget.min(u64::from(left.max(1)));
            }
        }
        self.step_unsettled(budget.min(u64::from(u32::MAX)) as u32);
        let spent = self.cycle_counter.wrapping_sub(before) as u64;
        if self.cgb.double_speed {
            spent
        } else {
            spent * 2
        }
    }

    fn takes_room(&self) -> bool {
        self.cpu.halted && self.serial.link_traffic_idle()
    }

    fn link_horizon(&self) -> u64 {
        if !self.cpu.halted
            || self.cpu.speed_switch_halt != 0
            || self.cgb.div_reset_pending
            || self.ic.has_pending_interrupt()
            || self.joypad.has_new_input()
            || !self.serial.link_silent()
        {
            return 0;
        }
        let dots = if self.cgb.double_speed { 2 } else { 4 };
        let cycles = (u32::from(self.ppu.quiet_dots()) / dots).min(self.timer.lazy_cycles());
        (u64::from(cycles) + 1) << u32::from(!self.cgb.double_speed)
    }

    fn linked_frame_done(&self) -> bool {
        self.ppu.frame_ready
            || (!self.ppu.lcdc.lcd_enabled && self.cycle_counter >= self.frame_cycles())
    }

    fn end_linked_frame(&mut self) {
        if self.cycle_counter >= self.frame_cycles() {
            self.cycle_counter -= self.frame_cycles();
        }
        self.settle_apu();
        self.apu.flush_audio();
    }
}

#[cfg(test)]
mod tests {
    use crate::gb::serial::LinkMessage;
    use crate::gb::{GameBoy, GbModel};
    use crate::test_support::{boot, read, write};

    const CYCLES_PER_MS: u64 = 1_048_576 / 1000;

    const DONE: u16 = 0xC0FF;

    const SWITCH_SPEED: [u8; 6] = [0x3E, 0x01, 0xE0, 0x4D, 0x10, 0x00];

    fn exchange_program(prefix: &[u8], sc: u8, first: u8, count: u8) -> Vec<u8> {
        let mut code = prefix.to_vec();
        code.extend_from_slice(&[
            0xAF, 0xEA, 0xFF, 0xC0, 0x21, 0x00, 0xC0, 0x06, count, 0x3E, first, 0xE0, 0x01, 0x4F,
            0x3E, sc, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA, 0xF0, 0x01, 0x22, 0x79, 0x3C,
            0x05, 0x20, 0xEB, 0x3E, 0xAA, 0xEA, 0xFF, 0xC0,
        ]);
        code
    }

    fn transfer(id: u16, byte: u8) -> LinkMessage {
        LinkMessage::Transfer {
            id,
            byte,
            seq: 0,
            since: 0,
        }
    }

    fn outgoing(gb: &mut GameBoy) -> Vec<LinkMessage> {
        gb.take_link_messages()
            .into_iter()
            .map(|message| match message {
                LinkMessage::Transfer { id, byte, .. } => transfer(id, byte),
                reply => reply,
            })
            .collect()
    }

    fn plugged(model: GbModel, cgb_flag: u8, code: &[u8]) -> GameBoy {
        let mut gb = boot(model, cgb_flag, code);
        gb.plug_link();
        gb
    }

    struct Pair {
        gbs: [GameBoy; 2],
        clocks: [u64; 2],
        waited: [u64; 2],
        longest_wait: [u64; 2],
        wait_run: [u64; 2],
        sent: [u64; 2],
        transfers: [Vec<(u64, u8)>; 2],
        in_flight: Vec<(u64, usize, LinkMessage)>,
        latency: u64,
        jitter: u64,
        noise: u64,
        last_due: [u64; 2],
    }

    impl Pair {
        fn new(a: GameBoy, b: GameBoy, latency_ms: u64) -> Self {
            Self {
                gbs: [a, b],
                clocks: [0; 2],
                waited: [0; 2],
                longest_wait: [0; 2],
                wait_run: [0; 2],
                sent: [0; 2],
                transfers: [Vec::new(), Vec::new()],
                in_flight: Vec::new(),
                latency: latency_ms * CYCLES_PER_MS,
                jitter: 0,
                noise: 0x9E37_79B9_7F4A_7C15,
                last_due: [0; 2],
            }
        }

        fn step(&mut self) {
            let i = if self.clocks[0] <= self.clocks[1] {
                0
            } else {
                1
            };
            let now = self.clocks[i];
            let mut arrived = Vec::new();
            self.in_flight.retain(|&(due, to, message)| {
                let ready = to == i && due <= now;
                if ready {
                    arrived.push(message);
                }
                !ready
            });
            for message in arrived {
                self.gbs[i].receive_link_message(message);
            }

            let gb = &mut self.gbs[i];
            let waiting = gb.link_waiting();
            let before = gb.cycle_counter;
            gb.step();
            let spent = gb.cycle_counter.wrapping_sub(before) as u64;
            self.clocks[i] += spent;
            if waiting {
                self.waited[i] += spent;
                self.wait_run[i] += spent;
                self.longest_wait[i] = self.longest_wait[i].max(self.wait_run[i]);
            } else {
                self.wait_run[i] = 0;
            }
            let played = self.clocks[i] - self.waited[i];
            for message in gb.take_link_messages() {
                self.sent[i] += 1;
                if let LinkMessage::Transfer { byte, .. } = message {
                    self.transfers[i].push((played, byte));
                }
                let mut due = self.clocks[i] + self.latency;
                if self.jitter > 0 {
                    self.noise ^= self.noise << 13;
                    self.noise ^= self.noise >> 7;
                    self.noise ^= self.noise << 17;
                    due = due - self.jitter + self.noise % (2 * self.jitter + 1);
                }
                let due = due.max(self.last_due[1 - i]);
                self.last_due[1 - i] = due;
                self.in_flight.push((due, 1 - i, message));
            }
        }

        fn run_until(&mut self, limit: u64, mut done: impl FnMut(&mut Pair) -> bool) {
            while !done(self) {
                assert!(
                    self.clocks[0].min(self.clocks[1]) < limit,
                    "pair did not finish within {limit} cycles"
                );
                self.step();
            }
        }

        fn with_jitter(mut self, jitter_ms: u64) -> Self {
            self.jitter = jitter_ms * CYCLES_PER_MS;
            self
        }

        fn played(&self, side: usize) -> u64 {
            self.clocks[side] - self.waited[side]
        }

        fn both_done(&mut self) -> bool {
            self.gbs.iter_mut().all(|gb| read(gb, DONE) == 0xAA)
        }
    }

    fn received(gb: &mut GameBoy, count: u8) -> Vec<u8> {
        (0..count as u16).map(|i| read(gb, 0xC000 + i)).collect()
    }

    fn run_exchange(master: GameBoy, slave: GameBoy, latency_ms: u64, count: u8) -> Pair {
        let mut pair = Pair::new(master, slave, latency_ms);
        let limit = (count as u64 + 4) * (2 * latency_ms * CYCLES_PER_MS + 20_000) + 200_000;
        pair.run_until(limit, Pair::both_done);
        assert_eq!(
            received(&mut pair.gbs[0], count),
            (0..count).map(|i| 0x80 + i).collect::<Vec<_>>()
        );
        assert_eq!(
            received(&mut pair.gbs[1], count),
            (0..count).map(|i| 0x10 + i).collect::<Vec<_>>()
        );
        assert!(pair.in_flight.is_empty());
        pair
    }

    #[test]
    fn scripted_bytes_cross_both_ways_at_any_latency() {
        for latency in [0, 16, 100, 250] {
            let count = 6;
            let master = plugged(
                GbModel::Dmg,
                0x00,
                &exchange_program(&[], 0x81, 0x10, count),
            );
            let slave = plugged(
                GbModel::Dmg,
                0x00,
                &exchange_program(&[], 0x80, 0x80, count),
            );
            let pair = run_exchange(master, slave, latency, count);

            let round_trip = 2 * latency * CYCLES_PER_MS;
            let transfer = 8 * 128;
            if latency == 0 {
                assert!(
                    pair.waited[0] < transfer,
                    "master paused {} cycles with no latency",
                    pair.waited[0]
                );
            } else {
                let least = count as u64 * (round_trip - transfer);
                let most = count as u64 * (round_trip + 5_000);
                assert!(
                    (least..most).contains(&pair.waited[0]),
                    "latency {latency}: master waited {} cycles, expected {least}..{most}",
                    pair.waited[0]
                );
            }
            assert_eq!(pair.waited[1], 0, "the slave never pauses");
        }
    }

    #[test]
    fn cgb_fast_clock_and_double_speed_exchange() {
        let count = 5;
        for (prefix, sc) in [
            (&[][..], 0x83),
            (&SWITCH_SPEED[..], 0x81),
            (&SWITCH_SPEED[..], 0x83),
        ] {
            for latency in [0, 16] {
                let master = plugged(
                    GbModel::Cgb,
                    0x80,
                    &exchange_program(prefix, sc, 0x10, count),
                );
                let slave = plugged(
                    GbModel::Cgb,
                    0x80,
                    &exchange_program(&[], 0x80, 0x80, count),
                );
                let pair = run_exchange(master, slave, latency, count);
                assert_eq!(pair.gbs[0].cgb.double_speed, !prefix.is_empty());
            }
        }
    }

    #[test]
    fn master_pauses_at_the_transfer_end_until_the_reply() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF0F, 0x00);
        write(&mut gb, 0xFF01, 0x42);
        write(&mut gb, 0xFF02, 0x81);
        assert_eq!(outgoing(&mut gb), vec![transfer(0, 0x42)]);
        while !gb.link_waiting() {
            gb.step();
        }
        let pc = gb.cpu.pc;
        for _ in 0..10_000 {
            gb.step();
        }
        assert_eq!(gb.cpu.pc, pc);
        assert_eq!(read(&mut gb, 0xFF02) & 0x80, 0x80);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x00);

        gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0x3C });
        assert!(!gb.link_waiting());
        assert_eq!(read(&mut gb, 0xFF01), 0x3C);
        assert_eq!(read(&mut gb, 0xFF02), 0x7F);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
    }

    #[test]
    fn a_flood_of_peer_transfers_stays_bounded() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        for id in 0..50_000u16 {
            gb.receive_link_message(transfer(id, 0x42));
        }
        assert!(gb.serial.link.held.len() <= super::HELD_LIMIT);
        assert_eq!(gb.serial.link.held.back().map(|held| held.id), Some(49_999));
    }

    #[test]
    fn early_reply_shifts_in_on_the_normal_clock() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF0F, 0x00);
        write(&mut gb, 0xFF01, 0x42);
        write(&mut gb, 0xFF02, 0x81);
        gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0xA5 });
        crate::test_support::cycles(&mut gb, 8 * 128 - 130);
        assert_eq!(read(&mut gb, 0xFF02) & 0x80, 0x80);
        crate::test_support::cycles(&mut gb, 130);
        assert_eq!(read(&mut gb, 0xFF02), 0x7F);
        assert_eq!(read(&mut gb, 0xFF01), 0xA5);
        assert!(!gb.link_waiting());
    }

    #[test]
    fn run_frame_returns_while_waiting_and_times_out_to_ff() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        gb.set_link_timeout_frames(3);
        write(&mut gb, 0xFF0F, 0x00);
        write(&mut gb, 0xFF02, 0x81);
        while !gb.link_waiting() {
            gb.step();
        }
        let pc = gb.cpu.pc;
        gb.run_frame();
        gb.run_frame();
        assert!(gb.link_waiting());
        assert_eq!(gb.cpu.pc, pc);
        assert!(!gb.ppu.frame_ready);
        gb.run_frame();
        assert!(!gb.link_waiting());
        assert_eq!(read(&mut gb, 0xFF01), 0xFF);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);

        gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0x12 });
        assert_eq!(read(&mut gb, 0xFF01), 0xFF);
    }

    #[test]
    fn step_timeout_counts_frames_of_cycles() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        gb.set_link_timeout_frames(1);
        write(&mut gb, 0xFF02, 0x81);
        while !gb.link_waiting() {
            gb.step();
        }
        let mut steps = 0;
        while gb.link_waiting() {
            gb.step();
            steps += 1;
        }
        assert_eq!(steps, 17556);
        assert_eq!(read(&mut gb, 0xFF01), 0xFF);
    }

    #[test]
    fn unplugged_reads_ff_and_sends_nothing() {
        let mut gb = boot(GbModel::Dmg, 0x00, &[]);
        assert!(!gb.link_plugged());
        write(&mut gb, 0xFF01, 0x55);
        write(&mut gb, 0xFF02, 0x81);
        crate::test_support::cycles(&mut gb, 8 * 128);
        assert!(!gb.link_waiting());
        assert_eq!(read(&mut gb, 0xFF01), 0xFF);
        assert!(gb.take_link_messages().is_empty());

        gb.receive_link_message(transfer(3, 0x10));
        assert!(gb.take_link_messages().is_empty());
    }

    #[test]
    fn plugging_in_mid_transfer_still_reads_ff() {
        let mut gb = boot(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF02, 0x81);
        crate::test_support::cycles(&mut gb, 300);
        gb.plug_link();
        crate::test_support::cycles(&mut gb, 8 * 128);
        assert!(!gb.link_waiting());
        assert_eq!(read(&mut gb, 0xFF01), 0xFF);
        assert!(gb.take_link_messages().is_empty());
    }

    #[test]
    fn pulling_the_cable_mid_transfer_keeps_the_bits_already_in() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF0F, 0x00);
        write(&mut gb, 0xFF02, 0x81);
        gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0x0A });
        crate::test_support::cycles(&mut gb, 4 * 128 + 10);
        gb.unplug_link();
        crate::test_support::cycles(&mut gb, 4 * 128);
        assert!(!gb.link_waiting());
        assert_eq!(read(&mut gb, 0xFF01), 0x0F);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
    }

    #[test]
    fn pulling_the_cable_while_waiting_reads_ff_at_once() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF0F, 0x00);
        write(&mut gb, 0xFF02, 0x81);
        while !gb.link_waiting() {
            gb.step();
        }
        gb.unplug_link();
        assert!(!gb.link_waiting());
        assert_eq!(read(&mut gb, 0xFF01), 0xFF);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
        assert!(gb.take_link_messages().is_empty());
    }

    #[test]
    fn slave_waits_forever_once_the_cable_is_pulled() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF0F, 0x00);
        write(&mut gb, 0xFF01, 0x77);
        write(&mut gb, 0xFF02, 0x80);
        gb.unplug_link();
        gb.receive_link_message(transfer(0, 0x11));
        for _ in 0..5 {
            gb.run_frame();
        }
        assert_eq!(read(&mut gb, 0xFF02) & 0x80, 0x80);
        assert_eq!(read(&mut gb, 0xFF01), 0x77);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x00);

        gb.plug_link();
        gb.receive_link_message(transfer(1, 0x11));
        crate::test_support::cycles(&mut gb, 8 * 128);
        assert_eq!(read(&mut gb, 0xFF01), 0x11);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
        assert_eq!(
            gb.take_link_messages(),
            vec![LinkMessage::Reply { id: 1, byte: 0x77 }]
        );
    }

    #[test]
    fn slave_answers_with_its_sb_and_raises_the_interrupt() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF0F, 0x00);
        write(&mut gb, 0xFF01, 0x99);
        write(&mut gb, 0xFF02, 0x80);
        gb.receive_link_message(transfer(7, 0x21));
        crate::test_support::cycles(&mut gb, 8 * 128);
        assert_eq!(read(&mut gb, 0xFF01), 0x21);
        assert_eq!(read(&mut gb, 0xFF02), 0x7E);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
        assert_eq!(
            gb.take_link_messages(),
            vec![LinkMessage::Reply { id: 7, byte: 0x99 }]
        );
    }

    #[test]
    fn early_transfer_waits_briefly_for_the_slave_to_arm() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF01, 0x99);
        gb.receive_link_message(transfer(1, 0x21));
        crate::test_support::cycles(&mut gb, 1000);
        assert!(gb.take_link_messages().is_empty());
        write(&mut gb, 0xFF02, 0x80);
        crate::test_support::cycles(&mut gb, 8 * 128 - 10);
        assert!(gb.take_link_messages().is_empty());
        crate::test_support::cycles(&mut gb, 10);
        assert_eq!(read(&mut gb, 0xFF01), 0x21);
        assert_eq!(
            gb.take_link_messages(),
            vec![LinkMessage::Reply { id: 1, byte: 0x99 }]
        );
    }

    #[test]
    fn a_slave_that_arms_after_the_byte_was_due_still_takes_a_full_byte_time() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF01, 0x99);
        write(&mut gb, 0xFF02, 0x80);
        gb.receive_link_message(transfer(1, 0x10));
        crate::test_support::cycles(&mut gb, 8 * 128);
        assert_eq!(
            gb.take_link_messages(),
            vec![LinkMessage::Reply { id: 1, byte: 0x99 }]
        );
        write(&mut gb, 0xFF01, 0x24);
        gb.receive_link_message(LinkMessage::Transfer {
            id: 2,
            byte: 0x4D,
            seq: 1,
            since: 0,
        });
        crate::test_support::cycles(&mut gb, 2000);
        assert!(gb.take_link_messages().is_empty());
        write(&mut gb, 0xFF02, 0x80);
        crate::test_support::cycles(&mut gb, 8 * 128 - 10);
        assert!(gb.take_link_messages().is_empty());
        assert_eq!(read(&mut gb, 0xFF01), 0x24);
        crate::test_support::cycles(&mut gb, 10);
        assert_eq!(read(&mut gb, 0xFF01), 0x4D);
        assert_eq!(
            gb.take_link_messages(),
            vec![LinkMessage::Reply { id: 2, byte: 0x24 }]
        );
    }

    #[test]
    fn a_slave_that_never_arms_answers_zero() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF0F, 0x00);
        write(&mut gb, 0xFF01, 0x99);
        gb.receive_link_message(transfer(2, 0x21));
        crate::test_support::cycles(&mut gb, 5000);
        assert_eq!(
            gb.take_link_messages(),
            vec![LinkMessage::Reply { id: 2, byte: 0x00 }]
        );
        assert_eq!(read(&mut gb, 0xFF01), 0x99);
        assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x00);
    }

    fn election_program(wait: u16) -> Vec<u8> {
        let [lo, hi] = wait.to_le_bytes();
        vec![
            0xAF, 0xEA, 0xFF, 0xC0, 0x3E, 0x02, 0xE0, 0x01, 0x3E, 0x80, 0xE0, 0x02, 0x01, lo, hi,
            0xF0, 0x02, 0xCB, 0x7F, 0x28, 0x13, 0x0B, 0x78, 0xB1, 0x20, 0xF5, 0x3E, 0x01, 0xE0,
            0x01, 0x3E, 0x81, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA, 0xF0, 0x01, 0xEA,
            0x00, 0xC0, 0x3E, 0xAA, 0xEA, 0xFF, 0xC0,
        ]
    }

    #[test]
    fn the_side_that_times_out_first_becomes_master() {
        for latency in [0, 16, 100, 250] {
            let mut pair = Pair::new(
                plugged(GbModel::Dmg, 0x00, &election_program(0x0100)),
                plugged(GbModel::Dmg, 0x00, &election_program(0xFFFF)),
                latency,
            );
            pair.run_until(2_000_000, Pair::both_done);
            assert_eq!(read(&mut pair.gbs[0], 0xC000), 0x02, "latency {latency}");
            assert_eq!(read(&mut pair.gbs[1], 0xC000), 0x01, "latency {latency}");
            assert_eq!(read(&mut pair.gbs[1], 0xFF02) & 0x01, 0x00);
        }
    }

    #[test]
    fn both_masters_read_ff() {
        let count = 3;
        for latency in [0, 100] {
            let mut pair = Pair::new(
                plugged(
                    GbModel::Dmg,
                    0x00,
                    &exchange_program(&[], 0x81, 0x10, count),
                ),
                plugged(
                    GbModel::Dmg,
                    0x00,
                    &exchange_program(&[], 0x81, 0x80, count),
                ),
                latency,
            );
            pair.run_until(2_000_000, Pair::both_done);
            for gb in &mut pair.gbs {
                assert_eq!(received(gb, count), vec![0xFF; count as usize]);
            }
        }
    }

    #[test]
    fn a_late_reply_after_a_restart_is_ignored() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF01, 0x01);
        write(&mut gb, 0xFF02, 0x81);
        write(&mut gb, 0xFF01, 0x02);
        write(&mut gb, 0xFF02, 0x81);
        assert_eq!(
            outgoing(&mut gb),
            vec![transfer(0, 0x01), transfer(1, 0x02),]
        );
        gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0x33 });
        while !gb.link_waiting() {
            gb.step();
        }
        gb.receive_link_message(LinkMessage::Reply { id: 1, byte: 0x44 });
        assert_eq!(read(&mut gb, 0xFF01), 0x44);
    }

    #[test]
    fn soft_reset_keeps_the_cable_plugged() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF02, 0x81);
        while !gb.link_waiting() {
            gb.step();
        }
        gb.soft_reset();
        assert!(gb.link_plugged());
        assert!(!gb.link_waiting());
        assert!(gb.take_link_messages().is_empty());
    }

    #[cfg(feature = "persistence")]
    mod snapshot {
        use super::*;
        use crate::gb::serial::Serial;

        #[test]
        fn a_snapshot_taken_while_waiting_restores_unplugged() {
            let code = exchange_program(&[], 0x81, 0x10, 1);
            let mut gb = plugged(GbModel::Dmg, 0x00, &code);
            while !gb.link_waiting() {
                gb.step();
            }
            let pc = gb.cpu.pc;
            let dump = gb.dump_full(0).expect("dump");
            let rom = crate::test_support::rom(0x00, &code);
            let mut restored = GameBoy::from_dump(&dump, &rom, 0).expect("restore");

            assert!(!restored.link_plugged());
            assert!(!restored.link_waiting());
            assert_eq!(restored.cpu.pc, pc);
            restored.step();
            assert!(!restored.link_waiting());
            assert_eq!(read(&mut restored, 0xFF01), 0xFF);
            assert_eq!(read(&mut restored, 0xFF0F) & 0x08, 0x08);
            for _ in 0..10 {
                restored.run_frame();
            }
            assert_eq!(read(&mut restored, DONE), 0xAA);
            assert_eq!(read(&mut restored, 0xC000), 0xFF);
        }

        #[test]
        fn serial_state_round_trips() {
            let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
            write(&mut gb, 0xFF01, 0x5A);
            write(&mut gb, 0xFF02, 0x81);
            crate::test_support::cycles(&mut gb, 3 * 128 + 70);
            let bytes = rmp_serde::to_vec_named(&gb.serial).expect("encode");
            let restored: Serial = rmp_serde::from_slice(&bytes).expect("decode");
            assert_eq!(restored.sb, gb.serial.sb);
            assert_eq!(restored.read_sc(false), gb.serial.read_sc(false));
            assert!(restored.transfer_active());
            assert!(restored.plugged());
        }

        #[test]
        fn serial_state_from_before_the_link_cable_loads() {
            #[derive(serde::Serialize)]
            struct OldSerial {
                sb: u8,
                sc: u8,
                bits: u8,
                clock_high: bool,
            }
            let old = OldSerial {
                sb: 0x12,
                sc: 0x81,
                bits: 3,
                clock_high: true,
            };
            let bytes = rmp_serde::to_vec_named(&old).expect("encode");
            let serial: Serial = rmp_serde::from_slice(&bytes).expect("decode");
            assert_eq!(serial.sb, 0x12);
            assert!(serial.transfer_active());
            assert!(!serial.waiting());
            assert!(!serial.plugged());
        }
    }

    fn serial_send(sb: u8, sc: u8) -> [u8; 8] {
        [0x3E, sb, 0xE0, 0x01, 0x3E, sc, 0xE0, 0x02]
    }

    const SERIAL_WAIT: [u8; 6] = [0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA];

    fn store_sb(lo: u8) -> [u8; 5] {
        [0xF0, 0x01, 0xEA, lo, 0xC0]
    }

    fn delay(loops: u16) -> [u8; 8] {
        let [lo, hi] = loops.to_le_bytes();
        [0x01, lo, hi, 0x0B, 0x78, 0xB1, 0x20, 0xFB]
    }

    fn program(parts: &[&[u8]]) -> Vec<u8> {
        let mut code = vec![0xAF, 0xEA, 0xFF, 0xC0];
        for part in parts {
            code.extend_from_slice(part);
        }
        code.extend_from_slice(&[0x3E, 0xAA, 0xEA, 0xFF, 0xC0]);
        code
    }

    #[test]
    fn a_slave_that_turns_master_before_the_byte_lands_keeps_the_order() {
        for latency in [0, 2, 16, 50, 100, 250] {
            let initiator = program(&[
                &serial_send(0x11, 0x81),
                &SERIAL_WAIT,
                &store_sb(0x00),
                &serial_send(0x22, 0x80),
                &SERIAL_WAIT,
                &store_sb(0x01),
            ]);
            let switcher = program(&[
                &serial_send(0x33, 0x80),
                &delay(900),
                &store_sb(0x00),
                &serial_send(0x44, 0x81),
                &SERIAL_WAIT,
                &store_sb(0x01),
            ]);
            let mut pair = Pair::new(
                plugged(GbModel::Dmg, 0x00, &initiator),
                plugged(GbModel::Dmg, 0x00, &switcher),
                latency,
            );
            pair.run_until(4_000_000, Pair::both_done);
            let seen = if latency < 6 { 0x11 } else { 0x33 };
            assert_eq!(
                received(&mut pair.gbs[0], 2),
                vec![0x33, 0x44],
                "{latency} ms"
            );
            assert_eq!(
                received(&mut pair.gbs[1], 2),
                vec![seen, 0x22],
                "{latency} ms"
            );
            assert!(pair.in_flight.is_empty());
        }
    }

    #[test]
    fn a_slave_that_arms_a_little_late_still_gets_the_byte_at_any_latency() {
        for latency in [0, 16, 100, 250] {
            let master = program(&[&serial_send(0x5A, 0x81), &SERIAL_WAIT, &store_sb(0x00)]);
            let slave = program(&[
                &delay(300),
                &serial_send(0xA5, 0x80),
                &SERIAL_WAIT,
                &store_sb(0x00),
            ]);
            let mut pair = Pair::new(
                plugged(GbModel::Dmg, 0x00, &master),
                plugged(GbModel::Dmg, 0x00, &slave),
                latency,
            );
            pair.run_until(2_000_000, Pair::both_done);
            assert_eq!(read(&mut pair.gbs[0], 0xC000), 0xA5, "{latency} ms");
            assert_eq!(read(&mut pair.gbs[1], 0xC000), 0x5A, "{latency} ms");
        }
    }

    #[test]
    fn two_sides_both_listening_count_their_idle_frames() {
        let listener = program(&[&serial_send(0x00, 0x80), &SERIAL_WAIT, &store_sb(0x00)]);
        let mut pair = Pair::new(
            plugged(GbModel::Dmg, 0x00, &listener),
            plugged(GbModel::Dmg, 0x00, &listener),
            16,
        );
        let timeout = crate::gb::serial::DEFAULT_LINK_TIMEOUT_FRAMES;
        let end = (timeout as u64 + 10) * FRAME_CYCLES;
        while pair.played(0).min(pair.played(1)) < end {
            pair.step();
        }
        for gb in &mut pair.gbs {
            assert!(gb.link_listening_frames() > timeout);
            assert!(!gb.link_waiting());
        }
        assert_eq!(pair.sent, [0, 0]);

        write(&mut pair.gbs[0], 0xFF02, 0x81);
        assert_eq!(pair.gbs[0].link_listening_frames(), 0);
        pair.gbs[1].unplug_link();
        assert_eq!(pair.gbs[1].link_listening_frames(), 0);
    }

    const SNAKE: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/snake.gb");

    const FRAME_CYCLES: u64 = 17_556;

    const SNAKE_GO: u8 = 0x06;

    fn snake() -> GameBoy {
        let rom = crate::rom::Rom::new(SNAKE);
        let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).unwrap());
        gb.load_rom(&rom).unwrap();
        gb
    }

    fn play_linked_snakes(latency_ms: u64, jitter_ms: u64) {
        use crate::gb::joypad::JoypadState;
        let timeout = crate::gb::serial::DEFAULT_LINK_TIMEOUT_FRAMES as u64 * FRAME_CYCLES;
        let mut first = snake();
        for _ in 0..60 {
            first.run_frame();
        }
        first.plug_link();
        let mut second = snake();
        second.plug_link();
        let mut pair = Pair::new(first, second, latency_ms).with_jitter(jitter_ms);
        for frame in 0..900u64 {
            let start = frame >= 60 && frame % 60 < 5;
            if start {
                pair.gbs[1].joypad.press(JoypadState::START);
            } else {
                pair.gbs[1].joypad.release(JoypadState::START);
            }
            let end = (frame + 1) * FRAME_CYCLES;
            while pair.played(1) < end {
                pair.step();
            }
        }
        let go = pair.transfers[1]
            .iter()
            .filter(|&&(_, byte)| byte == SNAKE_GO)
            .count();
        assert!(go >= 5, "{latency_ms}±{jitter_ms} ms: {go} GO bytes");
        for side in 0..2 {
            let last = pair.transfers[side].last().map_or(0, |&(played, _)| played);
            assert!(
                last >= 750 * FRAME_CYCLES,
                "{latency_ms}±{jitter_ms} ms: side {side} went quiet at frame {}",
                last / FRAME_CYCLES
            );
            assert!(
                pair.longest_wait[side] < timeout / 2,
                "{latency_ms}±{jitter_ms} ms: side {side} waited {} cycles at once",
                pair.longest_wait[side]
            );
        }
    }

    #[test]
    fn two_linked_snakes_play_rounds_back_to_back_over_the_cable() {
        for latency in [0, 16, 50, 100, 250] {
            play_linked_snakes(latency, 0);
        }
        play_linked_snakes(100, 40);
    }

    const PETRIS: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/petris.gbc");

    fn petris() -> GameBoy {
        let rom = crate::rom::Rom::new(PETRIS);
        let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).unwrap());
        gb.load_rom(&rom).unwrap();
        gb.plug_link();
        gb
    }

    fn petris_keys(side: usize, frame: u64) -> crate::gb::joypad::JoypadState {
        use crate::gb::joypad::JoypadState;
        let tap = |at: u64| (at..at + 6).contains(&frame);
        let mut keys = JoypadState::empty();
        if tap(600) || (side == 0 && tap(900)) {
            keys |= JoypadState::START;
        }
        if side == 0 && tap(700) {
            keys |= JoypadState::DOWN;
        }
        if side == 0 && tap(750) {
            keys |= JoypadState::RIGHT;
        }
        if side == 0 && tap(800) {
            keys |= JoypadState::UP;
        }
        keys
    }

    fn play_linked_petris(latency_ms: u64, jitter_ms: u64) {
        use crate::gb::joypad::JoypadState;
        let mut early = petris();
        for _ in 0..37 {
            early.run_frame();
        }
        let mut pair = Pair::new(early, petris(), latency_ms).with_jitter(jitter_ms);
        let all = [
            JoypadState::START,
            JoypadState::DOWN,
            JoypadState::RIGHT,
            JoypadState::UP,
        ];
        while pair.played(0).min(pair.played(1)) < 1500 * FRAME_CYCLES {
            for side in 0..2 {
                let keys = petris_keys(side, pair.played(side) / FRAME_CYCLES);
                for key in all {
                    if keys.contains(key) {
                        pair.gbs[side].joypad.press(key);
                    } else {
                        pair.gbs[side].joypad.release(key);
                    }
                }
            }
            pair.step();
        }
        let kinds = |side: usize| -> Vec<u8> {
            let mut kinds: Vec<u8> = pair.transfers[side]
                .iter()
                .map(|&(_, byte)| byte & 0xF0)
                .collect();
            kinds.dedup();
            kinds
        };
        let (host, guest) = (kinds(0), kinds(1));
        let label = format!("{latency_ms}±{jitter_ms} ms");
        let seed = host.iter().position(|&kind| kind == 0x30);
        let start = host.iter().rposition(|&kind| kind == 0x50);
        assert!(
            seed.zip(start).is_some_and(|(seed, start)| seed < start),
            "{label}: host sent {host:x?}"
        );
        let joined = guest.iter().position(|&kind| kind == 0x40);
        assert!(
            joined.is_some_and(|joined| !guest[joined..].contains(&0x10)),
            "{label}: guest sent {guest:x?}"
        );
    }

    #[test]
    fn two_linked_petris_start_a_versus_game() {
        for latency in [0, 16, 50, 100, 250] {
            play_linked_petris(latency, 0);
        }
        play_linked_petris(100, 40);
    }
}
