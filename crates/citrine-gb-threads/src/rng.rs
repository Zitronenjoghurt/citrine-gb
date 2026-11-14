#[derive(Debug, Clone)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub(crate) fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_gives_the_same_shuffle_every_time_and_another_seed_another() {
        let shuffled = |seed| {
            let mut items: Vec<u32> = (0..16).collect();
            Rng::new(seed).shuffle(&mut items);
            items
        };
        assert_eq!(shuffled(7), shuffled(7));
        assert_ne!(shuffled(7), shuffled(8));
        let mut sorted = shuffled(7);
        sorted.sort();
        assert_eq!(sorted, (0..16).collect::<Vec<_>>());
    }
}
