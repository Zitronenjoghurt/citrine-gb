use citrine_gb_direct::{Direct, Note};
use citrine_gb_net::{PeerLink, PlayMsg, Signal};
use std::sync::atomic::{AtomicU64, Ordering};
use web_time::Instant;

static TAGS: AtomicU64 = AtomicU64::new(1);

pub(super) struct DirectLink {
    tag: u64,
    told: bool,
    theirs: bool,
    dropped: bool,
    attempt: u32,
    tried: bool,
    open: bool,
    heard: Vec<(u32, Note)>,
}

impl Default for DirectLink {
    fn default() -> Self {
        Self {
            tag: TAGS.fetch_add(1, Ordering::Relaxed),
            told: false,
            theirs: false,
            dropped: false,
            attempt: 0,
            tried: false,
            open: false,
            heard: Vec::new(),
        }
    }
}

impl DirectLink {
    pub(super) fn tag(&self) -> u64 {
        self.tag
    }

    pub(super) fn hear(&mut self, signal: Signal) {
        match signal {
            Signal::Want(on) => {
                self.dropped |= self.theirs && !on;
                self.theirs = on;
            }
            Signal::Offer { attempt, sdp } => self.heard.push((attempt, Note::Offer(sdp))),
            Signal::Answer { attempt, sdp } => self.heard.push((attempt, Note::Answer(sdp))),
            Signal::Candidate { attempt, line } => {
                self.heard.push((attempt, Note::Candidate(line)))
            }
        }
    }

    pub(super) fn note_out(&self, note: Note) -> PlayMsg {
        let attempt = self.attempt;
        PlayMsg::Direct(match note {
            Note::Offer(sdp) => Signal::Offer { attempt, sdp },
            Note::Answer(sdp) => Signal::Answer { attempt, sdp },
            Note::Candidate(line) => Signal::Candidate { attempt, line },
        })
    }

    pub(super) fn up(&mut self, link: &mut PeerLink, now: Instant) {
        self.open = true;
        link.direct_up(now);
    }

    pub(super) fn down(&mut self, link: &mut PeerLink, now: Instant) {
        self.open = false;
        link.direct_down(now);
    }

    pub(super) fn step(
        &mut self,
        want: bool,
        offerer: bool,
        link: &mut PeerLink,
        direct: &mut Direct,
        outbox: &mut Vec<PlayMsg>,
        now: Instant,
    ) {
        if self.told != want {
            self.told = want;
            outbox.push(PlayMsg::Direct(Signal::Want(want)));
        }
        if std::mem::take(&mut self.dropped) || !(want && self.theirs) {
            self.close(link, direct, now);
        }
        if !(want && self.theirs) {
            self.heard.clear();
            return;
        }
        for (attempt, note) in std::mem::take(&mut self.heard) {
            match note {
                Note::Offer(_) if !offerer && attempt > self.attempt => {
                    self.attempt = attempt;
                    self.tried = true;
                    direct.hear(self.tag, note);
                }
                Note::Answer(_) if offerer && attempt == self.attempt => {
                    direct.hear(self.tag, note)
                }
                Note::Candidate(_) if attempt == self.attempt => direct.hear(self.tag, note),
                _ => {}
            }
        }
        if offerer && (!self.tried || link.retry_due(now)) {
            self.tried = true;
            self.attempt += 1;
            direct.offer(self.tag);
        }
    }

    pub(super) fn close(&mut self, link: &mut PeerLink, direct: &mut Direct, now: Instant) {
        if self.tried {
            direct.close(self.tag);
            self.tried = false;
        }
        if self.open {
            self.down(link, now);
        }
    }
}
