#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rng(u32);

const MULTIPLIER: u32 = 0x0019_660D;
const INCREMENT: u32 = 0x3C6E_F35F;

impl Rng {
    pub(crate) fn new(seed: u32) -> Self {
        Self(seed)
    }

    pub(crate) fn state(self) -> u32 {
        self.0
    }

    pub(crate) fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        self.0
    }

    pub(crate) fn percent(&mut self) -> u32 {
        (self.next() >> 3) % 100
    }

    pub(crate) fn low_percent(&mut self) -> u32 {
        ((self.next() >> 3) & 0xFF) % 100
    }

    pub(crate) fn whole_percent(&mut self) -> u32 {
        self.next() % 100
    }

    pub(crate) fn low(&mut self, shift: u32) -> u32 {
        (self.next() & 0xFFFF) >> shift
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_follows_the_walkers_generator() {
        let mut rng = Rng::new(0);
        assert_eq!(
            [rng.next(), rng.next(), rng.next()],
            [0x3C6E_F35F, 0x4750_2932, 0xD1CC_F6E9]
        );
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        assert_eq!((a.percent(), a.low(2)), (b.percent(), b.low(2)));
        assert_eq!(a.state(), b.state());
    }
}
