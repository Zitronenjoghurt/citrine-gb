use super::Online;
use citrine_gb_net::{
    ARCHIVE_PREFIX_BYTES, ARCHIVE_PROTOCOL, ArchiveGame, ArchiveId, ArchiveKey, ClientMsg,
    ErrorCode, ServerMsg,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFailure {
    Refused(ErrorCode),
    Damaged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDownload {
    pub game: ArchiveGame,
    pub packed: Vec<u8>,
}

struct Active {
    game: ArchiveGame,
    opened: Option<(ArchiveKey, [u8; ARCHIVE_PREFIX_BYTES], u32)>,
    packed: Vec<u8>,
    next: u32,
    asked: bool,
}

#[derive(Default)]
pub(super) struct ArchiveState {
    games: Vec<ArchiveGame>,
    listed: Option<Vec<ArchiveGame>>,
    active: Option<Active>,
    done: Vec<ArchiveDownload>,
    failed: Vec<(ArchiveId, ArchiveFailure)>,
}

impl Online {
    pub(super) fn archive_reset(&mut self) {
        self.archive = ArchiveState::default();
    }

    pub(super) fn archive_disconnected(&mut self) {
        if let Some(active) = self.archive.active.as_mut() {
            active.asked = false;
            active.opened = None;
        }
    }

    pub(super) fn archive_connected(&mut self) {
        self.archive_ask();
    }

    fn speaks_archive(&self) -> bool {
        self.protocol >= ARCHIVE_PROTOCOL
            && self
                .info
                .as_ref()
                .is_some_and(|info| info.protocol >= ARCHIVE_PROTOCOL)
    }

    fn archive_ask(&mut self) {
        if !self.is_connected() || !self.speaks_archive() {
            return;
        }
        let Some(active) = self.archive.active.as_ref().filter(|active| !active.asked) else {
            return;
        };
        let get = ClientMsg::ArchiveGet {
            id: active.game.id,
            from_part: active.next,
        };
        if self.send(get)
            && let Some(active) = self.archive.active.as_mut()
        {
            active.asked = true;
        }
    }

    pub fn archive_games(&self) -> &[ArchiveGame] {
        &self.archive.games
    }

    pub fn take_archive_list(&mut self) -> Option<Vec<ArchiveGame>> {
        self.archive.listed.take()
    }

    pub fn archive_fetch(&mut self, id: ArchiveId) -> bool {
        if self
            .archive
            .active
            .as_ref()
            .is_some_and(|a| a.game.id == id)
        {
            return true;
        }
        let Some(game) = self.archive.games.iter().find(|g| g.id == id).cloned() else {
            return false;
        };
        if let Some(old) = self.archive.active.take()
            && old.asked
        {
            self.send(ClientMsg::ArchiveCancel(old.game.id));
        }
        self.archive.active = Some(Active {
            game,
            opened: None,
            packed: Vec::new(),
            next: 0,
            asked: false,
        });
        self.archive_ask();
        true
    }

    pub fn archive_cancel(&mut self, id: ArchiveId) {
        if let Some(active) = self.archive.active.take_if(|a| a.game.id == id)
            && active.asked
        {
            self.send(ClientMsg::ArchiveCancel(id));
        }
    }

    pub fn archive_progress(&self, id: ArchiveId) -> Option<(u64, u64)> {
        let active = self.archive.active.as_ref().filter(|a| a.game.id == id)?;
        let sealed = u64::from(active.next) * citrine_gb_net::ARCHIVE_SEALED_PART_BYTES as u64;
        Some((sealed.min(active.game.download), active.game.download))
    }

    pub fn take_archive_done(&mut self) -> Vec<ArchiveDownload> {
        std::mem::take(&mut self.archive.done)
    }

    pub fn take_archive_failed(&mut self) -> Vec<(ArchiveId, ArchiveFailure)> {
        std::mem::take(&mut self.archive.failed)
    }

    fn archive_fail(&mut self, id: ArchiveId, failure: ArchiveFailure) {
        if self.archive.active.take_if(|a| a.game.id == id).is_some() {
            self.archive.failed.push((id, failure));
        }
    }

    pub(super) fn archive_receive(&mut self, msg: ServerMsg) -> Option<ServerMsg> {
        match msg {
            ServerMsg::Archive(games) => {
                if let Some(active) = &self.archive.active
                    && !games.iter().any(|g| g.id == active.game.id)
                {
                    self.archive.active = None;
                }
                self.archive.games.clone_from(&games);
                self.archive.listed = Some(games);
                None
            }
            ServerMsg::ArchiveStart {
                id,
                key,
                prefix,
                parts,
            } => {
                if let Some(active) = self.archive.active.as_mut().filter(|a| a.game.id == id) {
                    active.opened = Some((key, prefix, parts));
                }
                None
            }
            ServerMsg::ArchivePart { id, index, data } => {
                let active = self.archive.active.as_mut().filter(|a| a.game.id == id)?;
                let (key, prefix, parts) = active.opened.as_ref()?;
                let parts = *parts;
                if index != active.next {
                    return None;
                }
                match key.open_part(prefix, index, &data) {
                    Ok(part) => {
                        active.packed.extend_from_slice(&part);
                        active.next += 1;
                    }
                    Err(_) => {
                        self.archive_fail(id, ArchiveFailure::Damaged);
                        return None;
                    }
                }
                if active.next >= parts
                    && let Some(done) = self.archive.active.take()
                {
                    self.archive.done.push(ArchiveDownload {
                        game: done.game,
                        packed: done.packed,
                    });
                }
                None
            }
            ServerMsg::ArchiveFailed { id, error } => {
                self.archive_fail(id, ArchiveFailure::Refused(error));
                None
            }
            other => Some(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Wire, push, sent, signed_in};
    use super::*;
    use citrine_gb_net::{ARCHIVE_SEALED_PART_BYTES, ArchiveConsole, RelayId};
    use std::cell::RefCell;
    use std::rc::Rc;

    const PREFIX: [u8; ARCHIVE_PREFIX_BYTES] = [7; ARCHIVE_PREFIX_BYTES];

    fn game(id: u8, download: u64) -> ArchiveGame {
        ArchiveGame {
            id: ArchiveId([id; 16]),
            name: format!("Game {id}"),
            console: ArchiveConsole::GameBoy,
            size: download,
            download,
            sha256: [id; 32],
            sha1: [id; 20],
            crc32: 0,
        }
    }

    fn part(key: &ArchiveKey, id: ArchiveId, index: u32, plain: &[u8]) -> ServerMsg {
        ServerMsg::ArchivePart {
            id,
            index,
            data: key.seal_part(&PREFIX, index, plain),
        }
    }

    fn start(key: &ArchiveKey, id: ArchiveId, parts: u32) -> ServerMsg {
        ServerMsg::ArchiveStart {
            id,
            key: key.clone(),
            prefix: PREFIX,
            parts,
        }
    }

    fn gets(wire: &Rc<RefCell<Wire>>) -> Vec<(ArchiveId, u32)> {
        sent(wire)
            .into_iter()
            .filter_map(|msg| match msg {
                ClientMsg::ArchiveGet { id, from_part } => Some((id, from_part)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_listed_game_downloads_part_by_part() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        let tetris = game(1, 2 * ARCHIVE_SEALED_PART_BYTES as u64);
        push(&wire, ServerMsg::Archive(vec![tetris.clone()]));
        online.update(now);
        assert_eq!(online.take_archive_list(), Some(vec![tetris.clone()]));
        assert!(online.archive_fetch(tetris.id));
        online.update(now);
        assert_eq!(gets(&wire), vec![(tetris.id, 0)]);
        let key = ArchiveKey([3; 32]);
        push(&wire, start(&key, tetris.id, 2));
        push(&wire, part(&key, tetris.id, 0, b"first "));
        online.update(now);
        assert_eq!(
            online.archive_progress(tetris.id),
            Some((ARCHIVE_SEALED_PART_BYTES as u64, tetris.download))
        );
        push(&wire, part(&key, tetris.id, 1, b"second"));
        online.update(now);
        assert_eq!(online.archive_progress(tetris.id), None);
        assert_eq!(
            online.take_archive_done(),
            vec![ArchiveDownload {
                game: tetris,
                packed: b"first second".to_vec(),
            }]
        );
    }

    #[test]
    fn a_dropped_line_resumes_from_the_next_part() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        let tetris = game(1, 3 * ARCHIVE_SEALED_PART_BYTES as u64);
        online.receive(ServerMsg::Archive(vec![tetris.clone()]), now);
        online.archive_fetch(tetris.id);
        let key = ArchiveKey([3; 32]);
        online.receive(start(&key, tetris.id, 3), now);
        online.receive(part(&key, tetris.id, 0, b"a"), now);
        online.archive_disconnected();
        online.receive(part(&key, tetris.id, 1, b"lost"), now);
        online.archive_connected();
        assert_eq!(gets(&wire), vec![(tetris.id, 0), (tetris.id, 1)]);
        online.receive(start(&key, tetris.id, 3), now);
        online.receive(part(&key, tetris.id, 1, b"b"), now);
        online.receive(part(&key, tetris.id, 2, b"c"), now);
        assert_eq!(online.take_archive_done()[0].packed, b"abc");
    }

    #[test]
    fn a_bad_part_or_a_refusal_ends_the_download() {
        let (mut online, _wire, now) = signed_in(&RelayId::random());
        let (one, two) = (game(1, 10), game(2, 10));
        online.receive(ServerMsg::Archive(vec![one.clone(), two.clone()]), now);
        online.archive_fetch(one.id);
        online.receive(start(&ArchiveKey([3; 32]), one.id, 1), now);
        online.receive(part(&ArchiveKey([4; 32]), one.id, 0, b"x"), now);
        online.archive_fetch(two.id);
        online.receive(
            ServerMsg::ArchiveFailed {
                id: two.id,
                error: ErrorCode::RateLimited,
            },
            now,
        );
        assert_eq!(
            online.take_archive_failed(),
            vec![
                (one.id, ArchiveFailure::Damaged),
                (two.id, ArchiveFailure::Refused(ErrorCode::RateLimited)),
            ]
        );
        assert!(online.take_archive_done().is_empty());
    }

    #[test]
    fn a_game_leaving_the_list_stops_its_download() {
        let (mut online, _wire, now) = signed_in(&RelayId::random());
        let tetris = game(1, 10);
        online.receive(ServerMsg::Archive(vec![tetris.clone()]), now);
        online.archive_fetch(tetris.id);
        online.receive(ServerMsg::Archive(Vec::new()), now);
        assert_eq!(online.archive_progress(tetris.id), None);
        assert!(!online.archive_fetch(tetris.id));
        assert_eq!(online.take_archive_list(), Some(Vec::new()));
    }

    #[test]
    fn leaving_the_relay_forgets_the_archive() {
        let (mut online, _wire, now) = signed_in(&RelayId::random());
        online.receive(ServerMsg::Archive(vec![game(1, 10)]), now);
        online.leave();
        assert!(online.archive_games().is_empty());
        assert_eq!(online.take_archive_list(), None);
    }
}
