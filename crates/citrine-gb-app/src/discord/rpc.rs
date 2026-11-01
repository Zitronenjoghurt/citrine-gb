use super::Presence;
use discord_rich_presence::activity::{Activity, Assets, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

const APP_ID: &str = "1566154183000850492";
const MIN_GAP: Duration = Duration::from_secs(15);
const RETRY: Duration = Duration::from_secs(30);
const MAX_BYTES: usize = 128;

#[derive(Default)]
pub struct Rpc {
    tx: Option<Sender<Option<Presence>>>,
    sent: Option<Option<Presence>>,
}

impl Rpc {
    pub fn show(&mut self, presence: Option<Presence>) {
        if self.sent.as_ref() == Some(&presence) {
            return;
        }
        if presence.is_some() && self.tx.is_none() {
            self.tx = spawn(APP_ID);
        }
        if let Some(tx) = &self.tx
            && tx.send(presence.clone()).is_err()
        {
            self.tx = None;
        }
        self.sent = Some(presence);
    }
}

fn spawn(id: &'static str) -> Option<Sender<Option<Presence>>> {
    let (tx, rx) = mpsc::channel();
    match std::thread::Builder::new()
        .name("discord".into())
        .spawn(move || run(id, rx))
    {
        Ok(_) => Some(tx),
        Err(err) => {
            log::warn!("could not start the Discord thread: {err}");
            None
        }
    }
}

fn run(id: &str, rx: Receiver<Option<Presence>>) {
    let mut client: Option<DiscordIpcClient> = None;
    let mut tried: Option<Instant> = None;
    let mut sent_at: Option<Instant> = None;
    let mut wanted: Option<Presence> = None;
    let mut dirty = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(presence) => {
                wanted = presence;
                dirty = true;
                while let Ok(presence) = rx.try_recv() {
                    wanted = presence;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if !dirty || sent_at.is_some_and(|at| at.elapsed() < MIN_GAP) {
            continue;
        }
        if client.is_none() {
            if wanted.is_none() {
                dirty = false;
                continue;
            }
            if tried.is_some_and(|at| at.elapsed() < RETRY) {
                continue;
            }
            tried = Some(Instant::now());
            let mut fresh = DiscordIpcClient::new(id);
            if fresh.connect().is_err() {
                continue;
            }
            client = Some(fresh);
        }
        let Some(open) = client.as_mut() else {
            continue;
        };
        let result = match &wanted {
            Some(presence) => open.set_activity(activity(presence)),
            None => open.clear_activity(),
        }
        .and_then(|()| open.recv().map(|_| ()));
        sent_at = Some(Instant::now());
        match result {
            Ok(()) => dirty = false,
            Err(err) => {
                log::info!("Discord went away: {err}");
                client = None;
            }
        }
    }
    if let Some(mut open) = client {
        let _ = open.clear_activity();
        let _ = open.close();
    }
}

fn activity(presence: &Presence) -> Activity<'_> {
    let image = presence.image.as_deref().unwrap_or("citrine");
    let mut activity = Activity::new().details(clip(&presence.details)).assets(
        Assets::new()
            .large_image(image)
            .large_text(clip(presence.hover.as_deref().unwrap_or("Citrine"))),
    );
    if let Some(state) = &presence.state {
        activity = activity.state(clip(state));
    }
    if let Some(start) = presence.started_at {
        activity = activity.timestamps(Timestamps::new().start(start as i64));
    }
    activity
}

fn clip(text: &str) -> &str {
    let mut end = text.len().min(MAX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_are_cut_at_a_whole_character() {
        let name = "é".repeat(100);
        let cut = clip(&name);
        assert_eq!(cut.len(), 128);
        assert_eq!(cut.chars().count(), 64);
        assert_eq!(clip("Tetris"), "Tetris");
    }
}
