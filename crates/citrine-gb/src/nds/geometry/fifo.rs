use std::collections::VecDeque;

use super::command;

pub const CAPACITY: usize = 256;
pub const HALF: usize = 128;

const PIPE: usize = 4;
const PIPE_REFILL: usize = 2;
const COMMANDS_PER_WORD: u8 = 4;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entry {
    pub command: u8,
    pub param: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "Stored", into = "Stored"))]
pub struct Fifo {
    entries: VecDeque<Entry>,
    pipe: usize,
    packed: u32,
    packed_left: u8,
    params_left: usize,
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    pipe: VecDeque<Entry>,
    fifo: VecDeque<Entry>,
    packed: u32,
    packed_left: u8,
    params_left: usize,
}

#[cfg(feature = "serde")]
impl From<Stored> for Fifo {
    fn from(stored: Stored) -> Self {
        let pipe = stored.pipe.len();
        let mut entries = stored.pipe;
        entries.extend(stored.fifo);
        Self {
            entries,
            pipe,
            packed: stored.packed,
            packed_left: stored.packed_left,
            params_left: stored.params_left,
        }
    }
}

#[cfg(feature = "serde")]
impl From<Fifo> for Stored {
    fn from(mut fifo: Fifo) -> Self {
        let rest = fifo.entries.split_off(fifo.pipe);
        Self {
            pipe: fifo.entries,
            fifo: rest,
            packed: fifo.packed,
            packed_left: fifo.packed_left,
            params_left: fifo.params_left,
        }
    }
}

impl std::hash::Hash for Fifo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for part in [0..self.pipe, self.pipe..self.entries.len()] {
            state.write_usize(part.len());
            self.entries.range(part).for_each(|entry| entry.hash(state));
        }
        self.packed.hash(state);
        self.packed_left.hash(state);
        self.params_left.hash(state);
    }
}

impl Fifo {
    pub fn write_packed(&mut self, value: u32) {
        if self.packed_left == 0 {
            if value == 0 {
                self.push(Entry::default());
                return;
            }
            self.packed = value;
            self.packed_left = COMMANDS_PER_WORD;
        } else {
            self.push(Entry {
                command: self.packed as u8,
                param: value,
            });
            self.params_left -= 1;
            if self.params_left > 0 {
                return;
            }
            self.next_packed();
        }
        self.unpack();
    }

    pub fn write_port(&mut self, command: u8, value: u32) {
        self.push(Entry {
            command,
            param: value,
        });
    }

    pub fn pop(&mut self) -> Option<Entry> {
        let entry = self.entries.pop_front()?;
        self.pipe -= 1;
        if self.pipe <= PIPE_REFILL {
            self.pipe += (self.entries.len() - self.pipe).min(PIPE_REFILL);
        }
        Some(entry)
    }

    pub fn level(&self) -> usize {
        self.waiting().min(CAPACITY)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn full(&self) -> bool {
        self.waiting() >= CAPACITY
    }

    pub fn count(&self, command: impl Fn(u8) -> bool) -> usize {
        self.entries
            .iter()
            .filter(|entry| command(entry.command))
            .count()
    }

    pub fn drain(&self) -> Drain<'_> {
        Drain {
            entries: self.entries.iter(),
            pipe: self.pipe,
            fifo: self.waiting(),
        }
    }

    fn waiting(&self) -> usize {
        self.entries.len() - self.pipe
    }

    fn push(&mut self, entry: Entry) {
        if self.pipe == self.entries.len() && self.pipe < PIPE {
            self.pipe += 1;
        }
        self.entries.push_back(entry);
    }

    fn unpack(&mut self) {
        while self.packed_left > 0 {
            let command = self.packed as u8;
            let params = command::parameters(command);
            if params > 0 {
                self.params_left = params;
                return;
            }
            if command != command::NOP {
                self.push(Entry { command, param: 0 });
            }
            self.next_packed();
        }
    }

    fn next_packed(&mut self) {
        self.packed >>= 8;
        self.packed_left -= 1;
    }
}

pub struct Drain<'a> {
    entries: std::collections::vec_deque::Iter<'a, Entry>,
    pipe: usize,
    fifo: usize,
}

impl Drain<'_> {
    pub fn level(&self) -> usize {
        self.fifo.min(CAPACITY)
    }

    pub fn next_entry(&mut self) -> Option<Entry> {
        let entry = self.entries.next()?;
        self.pipe -= 1;
        if self.pipe <= PIPE_REFILL {
            let moved = self.fifo.min(PIPE_REFILL);
            self.pipe += moved;
            self.fifo -= moved;
        }
        Some(*entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(fifo: &mut Fifo) -> Vec<(u8, u32)> {
        std::iter::from_fn(|| fifo.pop())
            .map(|entry| (entry.command, entry.param))
            .collect()
    }

    #[test]
    fn port_write_is_one_entry() {
        let mut fifo = Fifo::default();
        fifo.write_port(0x10, 2);
        assert_eq!(drain(&mut fifo), [(0x10, 2)]);
    }

    #[test]
    fn packed_word_unpacks_each_command_with_its_parameters() {
        let mut fifo = Fifo::default();
        fifo.write_packed(0x2124_2210);
        for param in [1, 2, 3, 4] {
            fifo.write_packed(param);
        }
        assert_eq!(
            drain(&mut fifo),
            [(0x10, 1), (0x22, 2), (0x24, 3), (0x21, 4)]
        );
    }

    #[test]
    fn packed_parameterless_commands_need_no_parameter_word() {
        let mut fifo = Fifo::default();
        fifo.write_packed(0x0015_1110);
        fifo.write_packed(1);
        assert_eq!(drain(&mut fifo), [(0x10, 1), (0x11, 0), (0x15, 0)]);
    }

    #[test]
    fn packed_parameterless_commands_go_in_at_once() {
        let mut fifo = Fifo::default();
        fifo.write_packed(0x0015_1515);
        assert_eq!(drain(&mut fifo), [(0x15, 0); 3]);
    }

    #[test]
    fn packed_word_after_parameterless_commands_is_a_new_word() {
        let mut fifo = Fifo::default();
        fifo.write_packed(0x0000_0011);
        fifo.write_packed(0x0000_0010);
        fifo.write_packed(3);
        assert_eq!(drain(&mut fifo), [(0x11, 0), (0x10, 3)]);
    }

    #[test]
    fn packed_multi_parameter_command_takes_all_its_words() {
        let mut fifo = Fifo::default();
        fifo.write_packed(0x0000_1B10);
        for param in [0, 7, 8, 9] {
            fifo.write_packed(param);
        }
        assert_eq!(
            drain(&mut fifo),
            [(0x10, 0), (0x1B, 7), (0x1B, 8), (0x1B, 9)]
        );
    }

    #[test]
    fn packed_zero_bytes_are_skipped_inside_a_word_like_melonds() {
        let mut fifo = Fifo::default();
        fifo.write_packed(0x0015_0011);
        assert_eq!(drain(&mut fifo), [(0x11, 0), (0x15, 0)]);
    }

    #[test]
    fn packed_zero_word_is_one_nop_like_melonds() {
        let mut fifo = Fifo::default();
        fifo.write_packed(0);
        fifo.write_packed(0x0000_0010);
        fifo.write_packed(1);
        assert_eq!(drain(&mut fifo), [(0x00, 0), (0x10, 1)]);
    }

    #[test]
    fn packed_invalid_commands_fetch_no_parameters_per_gbatek() {
        let mut fifo = Fifo::default();
        fifo.write_packed(0x0000_1003);
        fifo.write_packed(5);
        assert_eq!(drain(&mut fifo), [(0x03, 0), (0x10, 5)]);
    }

    #[test]
    fn first_four_entries_sit_in_the_pipe() {
        let mut fifo = Fifo::default();
        for value in 0..4 {
            fifo.write_port(0x10, value);
        }
        assert_eq!(fifo.level(), 0);
        fifo.write_port(0x10, 4);
        assert_eq!(fifo.level(), 1);
    }

    #[test]
    fn pipe_takes_two_entries_when_it_drops_below_three() {
        let mut fifo = Fifo::default();
        for value in 0..10 {
            fifo.write_port(0x10, value);
        }
        assert_eq!(fifo.level(), 6);
        fifo.pop();
        assert_eq!(fifo.level(), 6);
        fifo.pop();
        assert_eq!(fifo.level(), 4);
    }

    #[test]
    fn full_at_256_entries_past_the_pipe() {
        let mut fifo = Fifo::default();
        for value in 0..259 {
            fifo.write_port(0x10, value);
        }
        assert!(!fifo.full());
        fifo.write_port(0x10, 259);
        assert!(fifo.full());
        assert_eq!(fifo.level(), 256);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn a_stored_fifo_keeps_the_pipe_apart_from_the_queue() {
        let mut fifo = Fifo::default();
        for value in 0..10 {
            fifo.write_port(0x10, value);
        }
        fifo.pop();
        fifo.pop();
        let stored = Stored::from(fifo.clone());
        assert_eq!(stored.pipe.len(), 4);
        assert_eq!(stored.fifo.len(), 4);
        assert_eq!(stored.pipe[0].param, 2);
        assert_eq!(stored.fifo[0].param, 6);
        assert_eq!(Fifo::from(stored), fifo);
    }
}
