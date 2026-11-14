use std::time::Duration;

const PERIOD_AGREES: f64 = 0.02;
const FALLBACK_REFRESHES: u64 = 2;
const NEXT_REFRESH: Duration = Duration::from_nanos(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wake {
    EveryRefresh,
    Idle,
    After(u64),
}

pub(crate) fn wake(busy: bool, idle_refreshes: Option<u64>, periods_agree: bool) -> Wake {
    if !busy {
        return Wake::Idle;
    }
    match idle_refreshes {
        Some(refreshes) if refreshes >= 2 && periods_agree => Wake::After(refreshes),
        _ => Wake::EveryRefresh,
    }
}

pub(crate) fn periods_agree(link: Option<f64>, measured: Option<f64>) -> bool {
    match (link, measured) {
        (Some(link), Some(measured)) if link > 0.0 => {
            ((measured - link) / link).abs() <= PERIOD_AGREES
        }
        _ => false,
    }
}

#[derive(Default)]
pub(crate) struct Repaint {
    #[cfg(target_os = "macos")]
    link: Option<crate::platform::vsync::VsyncLink>,
    #[cfg(target_os = "macos")]
    tried: bool,
    seen: Option<u64>,
    armed: bool,
}

impl Repaint {
    pub(crate) fn begin(&mut self, _ctx: &egui::Context) -> Option<u64> {
        let count = self.count(_ctx)?;
        let since = self
            .seen
            .replace(count)
            .map(|seen| count.saturating_sub(seen));
        let woken = std::mem::take(&mut self.armed);
        since.filter(|_| woken)
    }

    pub(crate) fn finish(
        &mut self,
        ctx: &egui::Context,
        busy: bool,
        idle_refreshes: Option<u64>,
        measured: Option<f64>,
        idle_tick: Duration,
    ) {
        let link_period = self.link_period();
        match wake(busy, idle_refreshes, periods_agree(link_period, measured)) {
            Wake::Idle => {
                self.arm(None);
                ctx.request_repaint_after(idle_tick);
            }
            Wake::EveryRefresh => {
                self.arm(None);
                ctx.request_repaint_after(NEXT_REFRESH);
            }
            Wake::After(refreshes) => {
                let period = link_period.unwrap_or_default();
                self.arm(self.seen.map(|seen| seen + refreshes));
                ctx.request_repaint_after(Duration::from_secs_f64(
                    period * (refreshes + FALLBACK_REFRESHES) as f64,
                ));
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn count(&mut self, ctx: &egui::Context) -> Option<u64> {
        if !std::mem::replace(&mut self.tried, true) {
            self.link = crate::platform::vsync::VsyncLink::start(ctx);
        }
        self.link.as_ref().map(|link| link.count())
    }

    #[cfg(not(target_os = "macos"))]
    fn count(&mut self, _ctx: &egui::Context) -> Option<u64> {
        None
    }

    #[cfg(target_os = "macos")]
    fn link_period(&self) -> Option<f64> {
        self.link.as_ref().and_then(|link| link.period())
    }

    #[cfg(not(target_os = "macos"))]
    fn link_period(&self) -> Option<f64> {
        None
    }

    #[cfg(target_os = "macos")]
    fn arm(&mut self, at: Option<u64>) {
        self.armed = at.is_some();
        if let Some(link) = &self.link {
            link.wake_at(at.unwrap_or(0));
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn arm(&mut self, at: Option<u64>) {
        self.armed = at.is_some();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_app_ticks_slowly() {
        assert_eq!(wake(false, Some(3), true), Wake::Idle);
    }

    #[test]
    fn a_game_frame_due_next_refresh_repaints_every_refresh() {
        assert_eq!(wake(true, Some(1), true), Wake::EveryRefresh);
        assert_eq!(wake(true, None, true), Wake::EveryRefresh);
    }

    #[test]
    fn refreshes_without_a_game_frame_are_skipped() {
        assert_eq!(wake(true, Some(2), true), Wake::After(2));
        assert_eq!(wake(true, Some(3), true), Wake::After(3));
    }

    #[test]
    fn a_frame_repainting_every_refresh_leaves_the_next_frame_free_to_wait() {
        let ctx = egui::Context::default();
        let mut repaint = Repaint::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            repaint.finish(ctx, true, Some(1), None, Duration::from_secs(1));
        });
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.request_repaint_after(Duration::from_millis(30));
        });
        let delay = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(delay > Duration::ZERO, "{delay:?}");
    }

    #[test]
    fn a_link_on_another_screen_rate_never_skips() {
        assert_eq!(wake(true, Some(2), false), Wake::EveryRefresh);
        assert!(periods_agree(Some(1.0 / 120.0), Some(1.0 / 119.0)));
        assert!(!periods_agree(Some(1.0 / 120.0), Some(1.0 / 60.0)));
        assert!(!periods_agree(None, Some(1.0 / 60.0)));
        assert!(!periods_agree(Some(1.0 / 60.0), None));
    }
}
