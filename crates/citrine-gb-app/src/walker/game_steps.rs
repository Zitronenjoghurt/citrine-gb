const MAX_JUMP: u32 = 64;

#[derive(Debug, Default)]
pub struct GameSteps {
    last: Option<u32>,
    owed: u32,
}

impl GameSteps {
    pub fn read(&mut self, counter: Option<u32>) {
        let Some(counter) = counter else {
            self.last = None;
            return;
        };
        if let Some(last) = self.last
            && counter > last
            && counter - last <= MAX_JUMP
        {
            self.owed = self.owed.saturating_add(counter - last);
        }
        self.last = Some(counter);
    }

    pub fn forget(&mut self) {
        self.last = None;
    }

    pub fn take(&mut self) -> u32 {
        std::mem::take(&mut self.owed)
    }

    pub fn owed(&self) -> u32 {
        self.owed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(steps: &mut GameSteps, counters: &[u32]) -> u32 {
        for &counter in counters {
            steps.read(Some(counter));
        }
        steps.take()
    }

    #[test]
    fn the_first_reading_counts_nothing() {
        let mut steps = GameSteps::default();
        assert_eq!(walk(&mut steps, &[5_000]), 0);
        assert_eq!(walk(&mut steps, &[5_001, 5_002, 5_002, 5_005]), 5);
    }

    #[test]
    fn a_counter_that_goes_back_counts_from_there() {
        let mut steps = GameSteps::default();
        assert_eq!(walk(&mut steps, &[100, 110]), 10);
        assert_eq!(walk(&mut steps, &[40, 42]), 2);
    }

    #[test]
    fn a_jump_from_a_loaded_state_counts_nothing() {
        let mut steps = GameSteps::default();
        assert_eq!(walk(&mut steps, &[100, 9_000, 9_003]), 3);
    }

    #[test]
    fn a_counter_out_of_sight_starts_over() {
        let mut steps = GameSteps::default();
        assert_eq!(walk(&mut steps, &[100, 101]), 1);
        steps.read(None);
        assert_eq!(walk(&mut steps, &[150, 151]), 1);
        steps.forget();
        assert_eq!(walk(&mut steps, &[200, 204]), 4);
    }

    #[test]
    fn steps_wait_until_taken() {
        let mut steps = GameSteps::default();
        steps.read(Some(1));
        steps.read(Some(3));
        steps.read(Some(4));
        assert_eq!(steps.owed(), 3);
        assert_eq!(steps.take(), 3);
        assert_eq!(steps.take(), 0);
    }
}
