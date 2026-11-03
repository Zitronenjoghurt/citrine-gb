use citrine_gb_net::OnlineId;
use std::collections::BTreeMap;
use std::time::Duration;
use web_time::Instant;

pub const SEND_EVERY: Duration = Duration::from_secs(3);
pub const SHOW_FOR: Duration = Duration::from_secs(5);
const REFUSAL_WINDOW: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Start(OnlineId),
    Stop(OnlineId),
}

impl Signal {
    pub fn to(self) -> (OnlineId, bool) {
        match self {
            Signal::Start(to) => (to, true),
            Signal::Stop(to) => (to, false),
        }
    }
}

#[derive(Debug)]
pub struct Typing {
    on: bool,
    draft: Option<(OnlineId, String)>,
    sent: Option<(OnlineId, Instant)>,
    heard: BTreeMap<OnlineId, Instant>,
    last_send: Option<Instant>,
    relay_lacks: bool,
}

impl Default for Typing {
    fn default() -> Self {
        Self {
            on: true,
            draft: None,
            sent: None,
            heard: BTreeMap::new(),
            last_send: None,
            relay_lacks: false,
        }
    }
}

impl Typing {
    pub fn set_on(&mut self, on: bool) -> Option<Signal> {
        self.on = on;
        if on {
            return None;
        }
        self.heard.clear();
        self.draft = None;
        self.stop()
    }

    pub fn compose(&mut self, draft: Option<(OnlineId, &str)>, now: Instant) -> Vec<Signal> {
        let mut out = Vec::new();
        if !self.on || self.relay_lacks {
            return out;
        }
        let writing = draft.filter(|(_, text)| !text.trim().is_empty());
        if let Some((to, _)) = self.sent
            && writing.is_none_or(|(friend, _)| friend != to)
        {
            out.extend(self.stop());
        }
        let last = self.draft.take();
        if let Some((friend, text)) = draft {
            let changed = last
                .as_ref()
                .is_some_and(|(was, before)| *was == friend && before != text);
            let due = self
                .sent
                .is_none_or(|(_, at)| now.duration_since(at) >= SEND_EVERY);
            if changed && writing.is_some() && due {
                self.sent = Some((friend, now));
                out.push(Signal::Start(friend));
            }
            self.draft = Some((friend, text.to_owned()));
        }
        out
    }

    pub fn stop(&mut self) -> Option<Signal> {
        self.sent.take().map(|(to, _)| Signal::Stop(to))
    }

    pub fn went_out(&mut self, now: Instant) {
        self.last_send = Some(now);
    }

    pub fn refused(&mut self, now: Instant) -> bool {
        let recent = self
            .last_send
            .is_some_and(|at| now.saturating_duration_since(at) < REFUSAL_WINDOW);
        if recent {
            self.relay_lacks = true;
            self.sent = None;
            self.draft = None;
        }
        recent
    }

    pub fn reset(&mut self) {
        self.sent = None;
        self.heard.clear();
        self.last_send = None;
        self.relay_lacks = false;
    }

    pub fn heard(&mut self, from: OnlineId, typing: bool, now: Instant) {
        if self.on && typing {
            self.heard.insert(from, now);
        } else {
            self.heard.remove(&from);
        }
    }

    pub fn gone(&mut self, from: OnlineId) {
        self.heard.remove(&from);
    }

    pub fn is_typing(&self, from: OnlineId, now: Instant) -> bool {
        self.on
            && self
                .heard
                .get(&from)
                .is_some_and(|at| now.saturating_duration_since(*at) < SHOW_FOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ana() -> OnlineId {
        OnlineId::new(7).unwrap()
    }

    fn ben() -> OnlineId {
        OnlineId::new(8).unwrap()
    }

    fn typed(t: &mut Typing, text: &str, at: Instant) -> Vec<Signal> {
        t.compose(Some((ana(), text)), at)
    }

    #[test]
    fn typing_sends_at_most_one_event_every_three_seconds() {
        let start = Instant::now();
        let mut t = Typing::default();
        assert!(typed(&mut t, "", start).is_empty());
        let mut sent = Vec::new();
        let mut text = String::new();
        for tenth in 1..=100u64 {
            text.push('a');
            let at = start + Duration::from_millis(tenth * 100);
            if !typed(&mut t, &text, at).is_empty() {
                sent.push(tenth);
            }
        }
        assert_eq!(sent, [1, 31, 61, 91]);
    }

    #[test]
    fn opening_a_chat_with_a_saved_draft_is_not_typing() {
        let now = Instant::now();
        let mut t = Typing::default();
        assert!(typed(&mut t, "half a thought", now).is_empty());
        assert!(typed(&mut t, "half a thought", now + SEND_EVERY * 2).is_empty());
        assert_eq!(
            typed(&mut t, "half a thought!", now + SEND_EVERY * 3),
            [Signal::Start(ana())]
        );
    }

    #[test]
    fn sending_emptying_or_closing_the_chat_stops() {
        let now = Instant::now();
        for end in [
            Some((ana(), "")),
            Some((ana(), "  ")),
            None,
            Some((ben(), "yo")),
        ] {
            let mut t = Typing::default();
            typed(&mut t, "", now);
            assert_eq!(typed(&mut t, "h", now), [Signal::Start(ana())]);
            assert_eq!(t.compose(end, now), [Signal::Stop(ana())]);
            assert!(t.compose(end, now).is_empty());
        }
    }

    #[test]
    fn a_friend_shows_until_a_stop_or_five_quiet_seconds() {
        let now = Instant::now();
        let mut t = Typing::default();
        t.heard(ana(), true, now);
        assert!(t.is_typing(ana(), now + SHOW_FOR - Duration::from_millis(1)));
        assert!(!t.is_typing(ana(), now + SHOW_FOR));
        assert!(!t.is_typing(ben(), now));
        t.heard(ana(), true, now + SEND_EVERY);
        assert!(t.is_typing(ana(), now + SHOW_FOR));
        t.heard(ana(), false, now + SHOW_FOR);
        assert!(!t.is_typing(ana(), now + SHOW_FOR));
    }

    #[test]
    fn off_neither_sends_nor_shows() {
        let now = Instant::now();
        let mut t = Typing::default();
        typed(&mut t, "", now);
        assert_eq!(typed(&mut t, "h", now), [Signal::Start(ana())]);
        t.heard(ben(), true, now);
        assert_eq!(t.set_on(false), Some(Signal::Stop(ana())));
        assert!(!t.is_typing(ben(), now));
        typed(&mut t, "", now);
        assert!(typed(&mut t, "hi", now + SEND_EVERY).is_empty());
        t.heard(ben(), true, now);
        assert!(!t.is_typing(ben(), now));
        assert_eq!(t.set_on(true), None);
        typed(&mut t, "hi", now);
        assert_eq!(typed(&mut t, "hi!", now), [Signal::Start(ana())]);
    }
}
