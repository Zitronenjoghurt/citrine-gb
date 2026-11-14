use super::Cpu;
use super::interrupt::Irq;

pub const FIRST: usize = 0x180;
pub const LAST: usize = 0x18B;
pub const RECEIVE_ADDRESS: u32 = 0x0410_0000;

pub const SYNC: usize = 0x0;
pub const CONTROL: usize = 0x4;
pub const SEND: usize = 0x8;

const FIFO_WORDS: usize = 16;

const SYNC_OUTPUT: u32 = 0x0F00;
const SYNC_SEND_IRQ: u32 = 1 << 13;
const SYNC_IRQ_ENABLE: u32 = 1 << 14;

const SEND_EMPTY: u32 = 1 << 0;
const SEND_FULL: u32 = 1 << 1;
const SEND_EMPTY_IRQ: u32 = 1 << 2;
const SEND_CLEAR: u32 = 1 << 3;
const RECEIVE_EMPTY: u32 = 1 << 8;
const RECEIVE_FULL: u32 = 1 << 9;
const RECEIVE_IRQ: u32 = 1 << 10;
const ERROR: u32 = 1 << 14;
const ENABLE: u32 = 1 << 15;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Fifo {
    words: [u32; FIFO_WORDS],
    head: usize,
    len: usize,
}

impl Fifo {
    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn is_full(&self) -> bool {
        self.len == FIFO_WORDS
    }

    fn front(&self) -> Option<u32> {
        (!self.is_empty()).then_some(self.words[self.head])
    }

    fn push(&mut self, word: u32) {
        self.words[(self.head + self.len) % FIFO_WORDS] = word;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<u32> {
        let word = self.front()?;
        self.head = (self.head + 1) % FIFO_WORDS;
        self.len -= 1;
        Some(word)
    }

    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Side {
    output: u32,
    sync_irq: bool,
    send_empty_irq: bool,
    receive_irq: bool,
    error: bool,
    enabled: bool,
    send: Fifo,
    last: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ipc {
    sides: [Side; 2],
    requests: [Irq; 2],
}

fn sides(cpu: Cpu) -> (usize, usize) {
    match cpu {
        Cpu::Arm9 => (0, 1),
        Cpu::Arm7 => (1, 0),
    }
}

fn bit(set: bool, bit: u32) -> u32 {
    if set { bit } else { 0 }
}

impl Ipc {
    pub fn read(&self, cpu: Cpu, offset: usize) -> u32 {
        let (own, other) = sides(cpu);
        let (own, other) = (&self.sides[own], &self.sides[other]);
        match offset & !3 {
            SYNC => (other.output >> 8) | own.output | bit(own.sync_irq, SYNC_IRQ_ENABLE),
            CONTROL => {
                bit(own.send.is_empty(), SEND_EMPTY)
                    | bit(own.send.is_full(), SEND_FULL)
                    | bit(own.send_empty_irq, SEND_EMPTY_IRQ)
                    | bit(other.send.is_empty(), RECEIVE_EMPTY)
                    | bit(other.send.is_full(), RECEIVE_FULL)
                    | bit(own.receive_irq, RECEIVE_IRQ)
                    | bit(own.error, ERROR)
                    | bit(own.enabled, ENABLE)
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, cpu: Cpu, offset: usize, value: u32, mask: u32) {
        match offset & !3 {
            SYNC => self.write_sync(cpu, value & mask, mask),
            CONTROL => self.write_control(cpu, value & mask, mask),
            SEND => self.send(cpu, value),
            _ => {}
        }
    }

    pub fn peek(&self, cpu: Cpu) -> u32 {
        let (own, other) = sides(cpu);
        self.sides[other]
            .send
            .front()
            .unwrap_or(self.sides[own].last)
    }

    pub fn receive(&mut self, cpu: Cpu) -> u32 {
        let (own, other) = sides(cpu);
        if !self.sides[own].enabled {
            return self.sides[other]
                .send
                .front()
                .unwrap_or(self.sides[own].last);
        }
        let Some(word) = self.sides[other].send.pop() else {
            self.sides[own].error = true;
            return self.sides[own].last;
        };
        self.sides[own].last = word;
        if self.sides[other].send.is_empty() && self.sides[other].send_empty_irq {
            self.requests[other] |= Irq::IPC_SEND_EMPTY;
        }
        word
    }

    pub fn take_requests(&mut self, cpu: Cpu) -> Irq {
        std::mem::take(&mut self.requests[sides(cpu).0])
    }

    fn write_sync(&mut self, cpu: Cpu, value: u32, mask: u32) {
        let (own, other) = sides(cpu);
        let side = &mut self.sides[own];
        side.output = (side.output & !(mask & SYNC_OUTPUT)) | (value & SYNC_OUTPUT);
        if mask & SYNC_IRQ_ENABLE != 0 {
            side.sync_irq = value & SYNC_IRQ_ENABLE != 0;
        }
        if value & SYNC_SEND_IRQ != 0 && self.sides[other].sync_irq {
            self.requests[other] |= Irq::IPC_SYNC;
        }
    }

    fn write_control(&mut self, cpu: Cpu, value: u32, mask: u32) {
        let (own, other) = sides(cpu);
        if value & SEND_CLEAR != 0 {
            self.sides[own].send.clear();
            self.sides[other].last = 0;
        }
        let receive_ready = !self.sides[other].send.is_empty();
        let side = &mut self.sides[own];
        if mask & SEND_EMPTY_IRQ != 0 {
            let enable = value & SEND_EMPTY_IRQ != 0;
            if enable && !side.send_empty_irq && side.send.is_empty() {
                self.requests[own] |= Irq::IPC_SEND_EMPTY;
            }
            side.send_empty_irq = enable;
        }
        if mask & RECEIVE_IRQ != 0 {
            let enable = value & RECEIVE_IRQ != 0;
            if enable && !side.receive_irq && receive_ready {
                self.requests[own] |= Irq::IPC_RECEIVE;
            }
            side.receive_irq = enable;
        }
        if value & ERROR != 0 {
            side.error = false;
        }
        if mask & ENABLE != 0 {
            side.enabled = value & ENABLE != 0;
        }
    }

    fn send(&mut self, cpu: Cpu, word: u32) {
        let (own, other) = sides(cpu);
        let side = &mut self.sides[own];
        if !side.enabled {
            return;
        }
        if side.send.is_full() {
            side.error = true;
            return;
        }
        let was_empty = side.send.is_empty();
        side.send.push(word);
        if was_empty && self.sides[other].receive_irq {
            self.requests[other] |= Irq::IPC_RECEIVE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = 0xFFFF_FFFF;

    fn enabled() -> Ipc {
        let mut ipc = Ipc::default();
        ipc.write(Cpu::Arm9, CONTROL, ENABLE, ALL);
        ipc.write(Cpu::Arm7, CONTROL, ENABLE, ALL);
        ipc
    }

    #[test]
    fn sync_output_shows_on_the_other_side() {
        let mut ipc = Ipc::default();
        ipc.write(Cpu::Arm9, SYNC, 0x0A00, ALL);
        assert_eq!(ipc.read(Cpu::Arm9, SYNC), 0x0A00);
        assert_eq!(ipc.read(Cpu::Arm7, SYNC), 0x000A);
        ipc.write(Cpu::Arm7, SYNC, 0x0305, ALL);
        assert_eq!(ipc.read(Cpu::Arm9, SYNC), 0x0A03);
        assert_eq!(ipc.read(Cpu::Arm7, SYNC), 0x030A);
    }

    #[test]
    fn sync_input_bits_are_read_only() {
        let mut ipc = Ipc::default();
        ipc.write(Cpu::Arm9, SYNC, 0x900F, ALL);
        assert_eq!(ipc.read(Cpu::Arm9, SYNC), 0);
    }

    #[test]
    fn sync_byte_write_keeps_the_other_byte() {
        let mut ipc = Ipc::default();
        ipc.write(Cpu::Arm7, SYNC, 0x4500, 0xFF00);
        ipc.write(Cpu::Arm7, SYNC, 0x0000, 0x00FF);
        assert_eq!(ipc.read(Cpu::Arm7, SYNC), 0x4500);
    }

    #[test]
    fn sync_irq_needs_the_remote_enable() {
        let mut ipc = Ipc::default();
        ipc.write(Cpu::Arm9, SYNC, SYNC_SEND_IRQ, ALL);
        assert!(ipc.take_requests(Cpu::Arm7).is_empty());
        ipc.write(Cpu::Arm7, SYNC, SYNC_IRQ_ENABLE, ALL);
        assert_eq!(ipc.read(Cpu::Arm7, SYNC), SYNC_IRQ_ENABLE);
        ipc.write(Cpu::Arm9, SYNC, SYNC_SEND_IRQ, ALL);
        assert_eq!(ipc.take_requests(Cpu::Arm7), Irq::IPC_SYNC);
        assert!(ipc.take_requests(Cpu::Arm9).is_empty());
        assert!(ipc.take_requests(Cpu::Arm7).is_empty());
    }

    #[test]
    fn control_starts_with_both_fifos_empty() {
        let ipc = Ipc::default();
        assert_eq!(ipc.read(Cpu::Arm9, CONTROL), SEND_EMPTY | RECEIVE_EMPTY);
    }

    #[test]
    fn words_cross_in_order() {
        let mut ipc = enabled();
        ipc.write(Cpu::Arm9, SEND, 1, ALL);
        ipc.write(Cpu::Arm9, SEND, 2, ALL);
        assert_eq!(ipc.read(Cpu::Arm9, CONTROL), ENABLE | RECEIVE_EMPTY);
        assert_eq!(ipc.read(Cpu::Arm7, CONTROL), ENABLE | SEND_EMPTY);
        assert_eq!(ipc.receive(Cpu::Arm7), 1);
        assert_eq!(ipc.receive(Cpu::Arm7), 2);
        assert_eq!(ipc.read(Cpu::Arm7, CONTROL) & ERROR, 0);
    }

    #[test]
    fn sixteen_words_fill_and_the_next_one_errors() {
        let mut ipc = enabled();
        for word in 0..16 {
            ipc.write(Cpu::Arm7, SEND, word, ALL);
        }
        assert_eq!(
            ipc.read(Cpu::Arm7, CONTROL),
            ENABLE | SEND_FULL | RECEIVE_EMPTY
        );
        assert_eq!(
            ipc.read(Cpu::Arm9, CONTROL),
            ENABLE | SEND_EMPTY | RECEIVE_FULL
        );
        ipc.write(Cpu::Arm7, SEND, 99, ALL);
        assert_ne!(ipc.read(Cpu::Arm7, CONTROL) & ERROR, 0);
        for word in 0..16 {
            assert_eq!(ipc.receive(Cpu::Arm9), word);
        }
        ipc.write(Cpu::Arm7, CONTROL, ENABLE | ERROR, ALL);
        assert_eq!(ipc.read(Cpu::Arm7, CONTROL) & ERROR, 0);
    }

    #[test]
    fn empty_read_returns_the_last_word_and_errors() {
        let mut ipc = enabled();
        assert_eq!(ipc.receive(Cpu::Arm9), 0);
        assert_ne!(ipc.read(Cpu::Arm9, CONTROL) & ERROR, 0);
        ipc.write(Cpu::Arm7, SEND, 0x1234, ALL);
        assert_eq!(ipc.receive(Cpu::Arm9), 0x1234);
        assert_eq!(ipc.receive(Cpu::Arm9), 0x1234);
    }

    #[test]
    fn send_clear_empties_and_zeroes_the_last_word() {
        let mut ipc = enabled();
        ipc.write(Cpu::Arm9, SEND, 7, ALL);
        ipc.write(Cpu::Arm9, SEND, 8, ALL);
        assert_eq!(ipc.receive(Cpu::Arm7), 7);
        ipc.write(Cpu::Arm9, CONTROL, ENABLE | SEND_CLEAR, ALL);
        assert_eq!(
            ipc.read(Cpu::Arm9, CONTROL),
            ENABLE | SEND_EMPTY | RECEIVE_EMPTY
        );
        assert_eq!(ipc.receive(Cpu::Arm7), 0);
    }

    #[test]
    fn disabled_fifo_drops_sends_and_peeks_reads() {
        let mut ipc = Ipc::default();
        ipc.write(Cpu::Arm9, SEND, 5, ALL);
        assert_eq!(ipc.read(Cpu::Arm9, CONTROL), SEND_EMPTY | RECEIVE_EMPTY);
        ipc.write(Cpu::Arm9, CONTROL, ENABLE, ALL);
        ipc.write(Cpu::Arm9, SEND, 5, ALL);
        assert_eq!(ipc.receive(Cpu::Arm7), 5);
        assert_eq!(ipc.receive(Cpu::Arm7), 5);
        assert_eq!(ipc.read(Cpu::Arm7, CONTROL), SEND_EMPTY);
    }

    #[test]
    fn receive_irq_fires_when_the_fifo_stops_being_empty() {
        let mut ipc = enabled();
        ipc.write(Cpu::Arm7, CONTROL, ENABLE | RECEIVE_IRQ, ALL);
        ipc.write(Cpu::Arm9, SEND, 1, ALL);
        assert_eq!(ipc.take_requests(Cpu::Arm7), Irq::IPC_RECEIVE);
        ipc.write(Cpu::Arm9, SEND, 2, ALL);
        assert!(ipc.take_requests(Cpu::Arm7).is_empty());
    }

    #[test]
    fn receive_irq_fires_on_enable_with_words_waiting() {
        let mut ipc = enabled();
        ipc.write(Cpu::Arm9, SEND, 1, ALL);
        ipc.write(Cpu::Arm7, CONTROL, ENABLE | RECEIVE_IRQ, ALL);
        assert_eq!(ipc.take_requests(Cpu::Arm7), Irq::IPC_RECEIVE);
        ipc.write(Cpu::Arm7, CONTROL, ENABLE | RECEIVE_IRQ, ALL);
        assert!(ipc.take_requests(Cpu::Arm7).is_empty());
    }

    #[test]
    fn send_empty_irq_fires_when_the_last_word_is_read() {
        let mut ipc = enabled();
        ipc.write(Cpu::Arm9, SEND, 1, ALL);
        ipc.write(Cpu::Arm9, SEND, 2, ALL);
        ipc.write(Cpu::Arm9, CONTROL, ENABLE | SEND_EMPTY_IRQ, ALL);
        assert!(ipc.take_requests(Cpu::Arm9).is_empty());
        ipc.receive(Cpu::Arm7);
        assert!(ipc.take_requests(Cpu::Arm9).is_empty());
        ipc.receive(Cpu::Arm7);
        assert_eq!(ipc.take_requests(Cpu::Arm9), Irq::IPC_SEND_EMPTY);
    }

    #[test]
    fn send_empty_irq_fires_on_enable_when_empty() {
        let mut ipc = enabled();
        ipc.write(Cpu::Arm7, CONTROL, ENABLE | SEND_EMPTY_IRQ, ALL);
        assert_eq!(ipc.take_requests(Cpu::Arm7), Irq::IPC_SEND_EMPTY);
    }

    #[test]
    fn fifo_wraps_around() {
        let mut ipc = enabled();
        for word in 0..40 {
            ipc.write(Cpu::Arm9, SEND, word, ALL);
            assert_eq!(ipc.receive(Cpu::Arm7), word);
        }
    }
}
