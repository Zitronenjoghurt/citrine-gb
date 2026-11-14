use std::fmt::Write;

pub const KEPT_BYTES: usize = 12;
pub const DEFAULT_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    AuxSpi,
    AuxControl,
    IrSent,
    IrReceived,
    CardCommand,
    IpcSync,
    IpcControl,
    IpcSend,
    Spi,
    SoftReset,
    SerialControl,
    SerialDone,
    WifiSent,
    WifiReceived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ended {
    HoldOff,
    ControlWrite,
    Reset,
    Open,
}

impl Ended {
    fn tag(self) -> u8 {
        self as u8
    }

    fn from_tag(tag: u8) -> Self {
        match tag {
            0 => Self::HoldOff,
            1 => Self::ControlWrite,
            2 => Self::Reset,
            _ => Self::Open,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Self::HoldOff => "hold off",
            Self::ControlWrite => "control write",
            Self::Reset => "reset",
            Self::Open => "still selected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub frame: u64,
    pub at: u64,
    pub last_frame: u64,
    pub kind: Kind,
    pub tag: u8,
    pub detail: u8,
    pub length: u32,
    pub repeats: u32,
    pub sent: [u8; KEPT_BYTES],
    pub got: [u8; KEPT_BYTES],
}

impl Entry {
    pub fn new(kind: Kind) -> Self {
        Self {
            frame: 0,
            at: 0,
            last_frame: 0,
            kind,
            tag: 0,
            detail: 0,
            length: 0,
            repeats: 1,
            sent: [0; KEPT_BYTES],
            got: [0; KEPT_BYTES],
        }
    }

    pub fn tag(mut self, tag: u8) -> Self {
        self.tag = tag;
        self
    }

    pub fn at(mut self, at: u64) -> Self {
        self.at = at;
        self
    }

    pub fn sent(mut self, bytes: &[u8]) -> Self {
        let kept = bytes.len().min(KEPT_BYTES);
        self.sent[..kept].copy_from_slice(&bytes[..kept]);
        self.length = self.length.max(bytes.len() as u32);
        self
    }

    pub fn got(mut self, bytes: &[u8]) -> Self {
        let kept = bytes.len().min(KEPT_BYTES);
        self.got[..kept].copy_from_slice(&bytes[..kept]);
        self.length = self.length.max(bytes.len() as u32);
        self
    }

    pub fn word(self, word: u32) -> Self {
        self.sent(&word.to_le_bytes())
    }

    fn same_as(&self, other: &Entry) -> bool {
        self.kind == other.kind
            && self.tag == other.tag
            && self.detail == other.detail
            && self.length == other.length
            && self.sent == other.sent
            && self.got == other.got
    }

    fn kept(&self) -> usize {
        (self.length as usize).min(KEPT_BYTES)
    }

    pub fn describe(&self) -> String {
        let mut text = String::new();
        let kept = self.kept();
        let sent = hex(&self.sent[..kept]);
        let got = hex(&self.got[..kept]);
        let more = if self.length as usize > KEPT_BYTES {
            " .."
        } else {
            ""
        };
        let word = u32::from_le_bytes([self.sent[0], self.sent[1], self.sent[2], self.sent[3]]);
        let _ = match self.kind {
            Kind::AuxSpi => write!(
                text,
                "auxspi {sent}{more} -> {got}{more} ({} bytes, ended by {})",
                self.length,
                Ended::from_tag(self.detail).describe()
            ),
            Kind::AuxControl => write!(text, "auxspicnt {:#06x}", word & 0xFFFF),
            Kind::IrSent => write!(text, "ir sent {} bytes: {sent}{more}", self.length),
            Kind::IrReceived => write!(text, "ir received {} bytes: {sent}{more}", self.length),
            Kind::CardCommand => write!(
                text,
                "card command {sent} for {} bytes",
                u32::from_le_bytes([self.got[0], self.got[1], self.got[2], self.got[3]])
            ),
            Kind::IpcSync => write!(text, "ipc {} sync {:#06x}", cpu(self.tag), word & 0xFFFF),
            Kind::IpcControl => {
                write!(
                    text,
                    "ipc {} fifo control {:#06x}",
                    cpu(self.tag),
                    word & 0xFFFF
                )
            }
            Kind::IpcSend => write!(text, "ipc {} sends {word:#010x}", cpu(self.tag)),
            Kind::Spi => write!(
                text,
                "spi {} {sent}{more} -> {got}{more} ({} bytes, ended by {})",
                spi_device(self.tag),
                self.length,
                Ended::from_tag(self.detail).describe()
            ),
            Kind::SoftReset => write!(text, "soft reset"),
            Kind::SerialControl => write!(text, "serial control {sent}"),
            Kind::SerialDone => write!(text, "serial done, sent {sent}, received {got}"),
            Kind::WifiSent => write!(
                text,
                "wifi sent {} bytes on channel {}: {sent}{more}",
                self.length, self.tag
            ),
            Kind::WifiReceived => write!(
                text,
                "wifi received {} bytes on channel {}: {sent}{more}",
                self.length, self.tag
            ),
        };
        if self.repeats > 1 {
            let _ = write!(text, " x{} until frame {}", self.repeats, self.last_frame);
        }
        text
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            text.push(' ');
        }
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn cpu(tag: u8) -> &'static str {
    if tag == 0 { "arm9" } else { "arm7" }
}

fn spi_device(tag: u8) -> &'static str {
    match tag {
        0 => "power",
        1 => "firmware",
        2 => "touch",
        _ => "reserved",
    }
}

pub struct Traffic {
    entries: Vec<Entry>,
    capacity: usize,
    next: usize,
    frame: u64,
    clock: u64,
    open: Option<Entry>,
}

impl Traffic {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity: capacity.max(1),
            next: 0,
            frame: 0,
            clock: 0,
            open: None,
        }
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn clock(&self) -> u64 {
        self.clock
    }

    pub fn set_clock(&mut self, clock: u64) {
        self.clock = clock;
    }

    pub fn next_frame(&mut self) {
        self.frame += 1;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push(&mut self, mut entry: Entry) {
        entry.frame = self.frame;
        entry.last_frame = self.frame;
        if let Some(last) = self.last_mut()
            && last.same_as(&entry)
        {
            last.repeats = last.repeats.saturating_add(1);
            last.last_frame = entry.frame;
            return;
        }
        if self.entries.len() < self.capacity {
            self.entries.push(entry);
        } else {
            self.entries[self.next] = entry;
            self.next = (self.next + 1) % self.capacity;
        }
    }

    pub fn byte(&mut self, kind: Kind, tag: u8, at: u64, sent: u8, got: u8) {
        let open = self
            .open
            .get_or_insert_with(|| Entry::new(kind).tag(tag).at(at));
        let index = open.length as usize;
        if index < KEPT_BYTES {
            open.sent[index] = sent;
            open.got[index] = got;
        }
        open.length = open.length.saturating_add(1);
    }

    pub fn close(&mut self, ended: Ended) {
        if let Some(mut open) = self.open.take() {
            open.detail = ended.tag();
            self.push(open);
        }
    }

    pub fn open(&self) -> Option<Entry> {
        self.open.map(|mut open| {
            open.frame = self.frame;
            open.last_frame = self.frame;
            open.detail = Ended::Open.tag();
            open
        })
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        let (newer, older) = self.entries.split_at(self.next);
        older.iter().chain(newer)
    }

    pub fn adopt(&mut self, old: &mut Traffic) {
        *self = std::mem::take(old);
    }

    fn last_mut(&mut self) -> Option<&mut Entry> {
        if self.entries.is_empty() {
            return None;
        }
        let last = if self.entries.len() < self.capacity {
            self.entries.len() - 1
        } else {
            (self.next + self.capacity - 1) % self.capacity
        };
        self.entries.get_mut(last)
    }
}

impl Default for Traffic {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl Clone for Traffic {
    fn clone(&self) -> Self {
        Self {
            frame: self.frame,
            ..Self::with_capacity(self.capacity)
        }
    }
}

impl std::fmt::Debug for Traffic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Traffic({} entries)", self.entries.len())
    }
}

impl PartialEq for Traffic {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for Traffic {}

impl std::hash::Hash for Traffic {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

#[derive(Debug, Clone, Copy)]
pub struct Ring<'a> {
    pub name: &'static str,
    pub traffic: &'a Traffic,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sends(traffic: &Traffic) -> Vec<u8> {
        traffic.entries().map(|entry| entry.sent[0]).collect()
    }

    #[test]
    fn the_ring_keeps_the_newest_entries_in_order_after_it_wraps() {
        let mut traffic = Traffic::with_capacity(4);
        for byte in 0..10u8 {
            traffic.push(Entry::new(Kind::IpcSend).sent(&[byte]));
        }
        assert_eq!(sends(&traffic), vec![6, 7, 8, 9]);
        assert_eq!(traffic.len(), 4);
    }

    #[test]
    fn identical_entries_in_a_row_collapse_into_one_with_a_count() {
        let mut traffic = Traffic::with_capacity(4);
        for _ in 0..3 {
            traffic.push(Entry::new(Kind::AuxSpi).sent(&[5, 0]).got(&[0xFF, 0]));
            traffic.next_frame();
        }
        traffic.push(Entry::new(Kind::AuxSpi).sent(&[3, 0]));
        let entries: Vec<_> = traffic.entries().copied().collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].repeats, 3);
        assert_eq!((entries[0].frame, entries[0].last_frame), (0, 2));
        assert!(entries[0].describe().ends_with("x3 until frame 2"));
    }

    #[test]
    fn repeats_collapse_across_the_wrap_point() {
        let mut traffic = Traffic::with_capacity(2);
        traffic.push(Entry::new(Kind::IpcSend).sent(&[1]));
        traffic.push(Entry::new(Kind::IpcSend).sent(&[2]));
        traffic.push(Entry::new(Kind::IpcSend).sent(&[3]));
        traffic.push(Entry::new(Kind::IpcSend).sent(&[3]));
        assert_eq!(sends(&traffic), vec![2, 3]);
        assert_eq!(traffic.entries().last().unwrap().repeats, 2);
    }

    #[test]
    fn a_transaction_gathers_its_bytes_until_it_closes() {
        let mut traffic = Traffic::default();
        traffic.byte(Kind::AuxSpi, 0, 7, 0x03, 0xFF);
        traffic.byte(Kind::AuxSpi, 0, 9, 0x00, 0xAA);
        assert!(traffic.is_empty());
        assert_eq!(traffic.open().unwrap().length, 2);
        traffic.close(Ended::HoldOff);
        let entry = *traffic.entries().next().unwrap();
        assert_eq!(entry.at, 7);
        assert_eq!(
            entry.describe(),
            "auxspi 03 00 -> ff aa (2 bytes, ended by hold off)"
        );
    }

    #[test]
    fn long_transactions_keep_their_first_bytes_and_their_length() {
        let mut traffic = Traffic::default();
        for byte in 0..40u8 {
            traffic.byte(Kind::Spi, 1, 0, byte, 0);
        }
        traffic.close(Ended::ControlWrite);
        let entry = traffic.entries().next().unwrap();
        assert_eq!(entry.length, 40);
        assert_eq!(entry.sent[KEPT_BYTES - 1], KEPT_BYTES as u8 - 1);
        assert!(entry.describe().starts_with("spi firmware 00 01"));
    }

    #[test]
    fn a_clone_starts_empty_and_keeps_the_frame() {
        let mut traffic = Traffic::default();
        traffic.next_frame();
        traffic.push(Entry::new(Kind::SoftReset));
        let copy = traffic.clone();
        assert!(copy.is_empty());
        assert_eq!(copy.frame(), 1);
    }
}
