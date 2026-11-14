use super::engine::Rollback;
use super::fetch::Fetch;
use citrine_gb::console::{Console, VideoSpec};
use std::time::Duration;
use web_time::Instant;

#[derive(Clone, Copy)]
pub struct Picture<'a> {
    pub video: VideoSpec,
    pub frame: &'a [u8],
}

impl<'a> Picture<'a> {
    pub fn of(console: &'a dyn Console) -> Self {
        Self {
            video: console.video(),
            frame: console.frame(),
        }
    }
}

#[derive(Default)]
pub(super) struct Still {
    video: Option<VideoSpec>,
    frame: Vec<u8>,
}

impl Still {
    pub(super) fn keep(&mut self, console: Option<&dyn Console>, watched: bool) {
        self.video = console.map(Console::video);
        let frame = console.map_or(&[][..], Console::frame);
        if watched || self.frame.len() != frame.len() {
            self.frame.clear();
            self.frame.extend_from_slice(frame);
        }
    }

    pub(super) fn picture(&self) -> Option<Picture<'_>> {
        self.video.map(|video| Picture {
            video,
            frame: &self.frame,
        })
    }
}

#[derive(Default)]
pub(super) struct Seen {
    friend: Still,
    group: bool,
    plugged: bool,
    stalled_since: Option<Instant>,
    waiting_unit: Option<u8>,
    fetch: Option<Fetch>,
}

impl Seen {
    pub(super) fn publish(&mut self, r: &Rollback) {
        self.friend.keep(r.console(false), r.watches());
        self.group = r.is_group();
        self.plugged = r.cable_plugged();
        self.stalled_since = r.stalled_since();
        self.waiting_unit = r.waiting_unit();
        self.fetch = r.fetch();
    }
}

#[derive(Clone, Copy)]
pub(super) enum Shown<'a> {
    Home(&'a Rollback),
    Away(&'a Seen),
}

impl<'a> Shown<'a> {
    pub(super) fn friend(self) -> Option<Picture<'a>> {
        match self {
            Self::Home(r) => r.console(false).map(Picture::of),
            Self::Away(seen) => seen.friend.picture(),
        }
    }

    pub(super) fn group(self) -> bool {
        match self {
            Self::Home(r) => r.is_group(),
            Self::Away(seen) => seen.group,
        }
    }

    pub(super) fn plugged(self) -> bool {
        match self {
            Self::Home(r) => r.cable_plugged(),
            Self::Away(seen) => seen.plugged,
        }
    }

    pub(super) fn stalled_for(self, now: Instant) -> Duration {
        let since = match self {
            Self::Home(r) => r.stalled_since(),
            Self::Away(seen) => seen.stalled_since,
        };
        since.map_or(Duration::ZERO, |at| now.duration_since(at))
    }

    pub(super) fn waiting_unit(self) -> Option<u8> {
        match self {
            Self::Home(r) => r.waiting_unit(),
            Self::Away(seen) => seen.waiting_unit,
        }
    }

    pub(super) fn fetch(self) -> Option<Fetch> {
        match self {
            Self::Home(r) => r.fetch(),
            Self::Away(seen) => seen.fetch.clone(),
        }
    }
}
