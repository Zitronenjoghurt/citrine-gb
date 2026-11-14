#[derive(Default)]
pub(super) struct Word(Option<(u32, u32)>);

impl Word {
    fn load(&mut self, address: u32, read: impl FnOnce(u32) -> u32) -> u32 {
        let base = address & !3;
        match self.0 {
            Some((at, word)) if at == base => word,
            _ => {
                let word = read(base);
                self.0 = Some((base, word));
                word
            }
        }
    }

    pub fn half(&mut self, address: u32, read: impl FnOnce(u32) -> u32) -> u16 {
        debug_assert_eq!(address & 1, 0);
        (self.load(address, read) >> ((address & 2) * 8)) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::Word;
    use std::cell::Cell;

    #[test]
    fn reads_once_per_aligned_word() {
        let memory: Vec<u8> = (0..16).collect();
        let reads = Cell::new(0);
        let read = |at: u32| {
            reads.set(reads.get() + 1);
            u32::from_le_bytes(memory[at as usize..at as usize + 4].try_into().unwrap())
        };
        let mut word = Word::default();
        let halves: Vec<u16> = (0..4).map(|at| word.half(at * 2, read)).collect();
        assert_eq!(halves, [0x0100, 0x0302, 0x0504, 0x0706]);
        assert_eq!(reads.get(), 2);
        assert_eq!(word.half(8, read), 0x0908);
        assert_eq!(word.half(2, read), 0x0302);
        assert_eq!(reads.get(), 4);
    }
}
