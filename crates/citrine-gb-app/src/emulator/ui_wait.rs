use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

const KEPT: usize = 32;
const TAIL: Duration = Duration::from_micros(1500);
const FASTEST: Duration = Duration::from_micros(2778);
const SLOWEST: Duration = Duration::from_micros(16_667);

#[derive(Debug, Default)]
pub(crate) struct UiWait {
    last: Option<Instant>,
    gaps: VecDeque<Duration>,
    deadline: Option<Instant>,
}

impl UiWait {
    pub(crate) fn begin(&mut self, now: Instant, refreshes: Option<u64>) {
        let parts = refreshes.unwrap_or(1);
        if parts > 0
            && let Some(last) = self.last.replace(now)
        {
            if self.gaps.len() == KEPT {
                self.gaps.pop_front();
            }
            let gap =
                now.saturating_duration_since(last) / u32::try_from(parts).unwrap_or(u32::MAX);
            self.gaps.push_back(gap);
        }
        self.deadline = Some(now + self.refresh().saturating_sub(TAIL));
    }

    pub(crate) fn refresh(&self) -> Duration {
        let mut gaps: Vec<Duration> = self.gaps.iter().copied().collect();
        gaps.sort_unstable();
        gaps.get(gaps.len() / 2)
            .copied()
            .map_or(SLOWEST, |gap| gap.clamp(FASTEST, SLOWEST))
    }

    pub(crate) fn first(&self) -> Duration {
        self.refresh().saturating_sub(TAIL)
    }

    pub(crate) fn left(&self, now: Instant, cap: Duration) -> Duration {
        self.deadline
            .map_or(cap, |deadline| deadline.saturating_duration_since(now))
            .min(cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: Duration = Duration::from_millis(4);

    fn ticks(wait: &mut UiWait, start: Instant, gap: Duration, count: u32) -> Instant {
        let mut now = start;
        for _ in 0..count {
            wait.begin(now, None);
            now += gap;
        }
        now - gap
    }

    #[test]
    fn before_any_frame_the_cap_holds() {
        let wait = UiWait::default();
        assert_eq!(wait.left(Instant::now(), CAP), CAP);
    }

    #[test]
    fn at_60_hz_the_wait_keeps_its_cap() {
        let mut wait = UiWait::default();
        let last = ticks(&mut wait, Instant::now(), Duration::from_micros(16_667), 10);
        assert_eq!(wait.left(last, CAP), CAP);
    }

    #[test]
    fn at_180_hz_the_wait_ends_before_the_refresh_does() {
        let mut wait = UiWait::default();
        let gap = Duration::from_micros(5556);
        let last = ticks(&mut wait, Instant::now(), gap, 10);
        let later = last + Duration::from_micros(500);
        assert_eq!(
            wait.left(later, CAP),
            gap - TAIL - Duration::from_micros(500)
        );
    }

    #[test]
    fn a_late_frame_does_not_stretch_the_refresh() {
        let mut wait = UiWait::default();
        let gap = Duration::from_micros(5556);
        let last = ticks(&mut wait, Instant::now(), gap, 10);
        wait.begin(last + Duration::from_millis(12), None);
        assert_eq!(wait.refresh(), gap);
    }

    #[test]
    fn a_few_early_updates_do_not_shrink_the_refresh() {
        let mut wait = UiWait::default();
        let gap = Duration::from_micros(5556);
        let mut last = ticks(&mut wait, Instant::now(), gap, 20);
        for _ in 0..5 {
            last += Duration::from_micros(300);
            wait.begin(last, None);
            last += gap;
            wait.begin(last, None);
        }
        assert_eq!(wait.refresh(), gap);
    }

    #[test]
    fn past_the_deadline_nothing_is_left() {
        let mut wait = UiWait::default();
        let last = ticks(&mut wait, Instant::now(), Duration::from_micros(5556), 10);
        assert_eq!(
            wait.left(last + Duration::from_millis(5), CAP),
            Duration::ZERO
        );
    }
}
