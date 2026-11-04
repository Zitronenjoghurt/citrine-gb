const CAPACITY: usize = 32;
const REFILL_AT: usize = 12;
const WORD: usize = 4;

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Fifo {
    bytes: [i8; CAPACITY],
    read: usize,
    len: usize,
    sample: i8,
    #[cfg_attr(feature = "serde", serde(default))]
    playing: usize,
}

impl Fifo {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn sample(&self) -> i8 {
        self.sample
    }

    pub fn tick(&mut self) -> bool {
        let queued = self.len - self.playing;
        if self.playing == 0 {
            self.playing = queued.min(WORD);
        }
        self.pop();
        queued <= REFILL_AT
    }

    pub fn push(&mut self, byte: u8) {
        self.bytes[(self.read + self.len) % CAPACITY] = byte as i8;
        self.len += 1;
        if self.len == CAPACITY {
            self.empty();
        }
    }

    fn empty(&mut self) {
        self.read = 0;
        self.len = 0;
        self.playing = 0;
    }

    fn pop(&mut self) {
        if self.playing == 0 {
            return;
        }
        self.sample = self.bytes[self.read];
        self.read = (self.read + 1) % CAPACITY;
        self.len -= 1;
        self.playing -= 1;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pushed(bytes: usize) -> Fifo {
        let mut fifo = Fifo::default();
        for byte in 0..bytes {
            fifo.push(byte as u8 + 1);
        }
        fifo
    }

    #[test]
    fn seven_words_stay_queued() {
        assert_eq!(pushed(28).len(), 28);
    }

    #[test]
    fn the_eighth_word_empties_the_fifo() {
        assert_eq!(pushed(32).len(), 0);
    }

    #[test]
    fn filling_it_after_a_sample_played_empties_it_and_keeps_the_sample() {
        let mut fifo = pushed(28);
        fifo.tick();
        for byte in 0..5 {
            fifo.push(byte);
        }
        assert_eq!(fifo.len(), 0);
        assert_eq!(fifo.sample(), 1);
    }
}
