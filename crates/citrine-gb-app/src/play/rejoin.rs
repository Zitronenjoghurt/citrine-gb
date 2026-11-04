use super::PlaySession;
use super::engine::Rollback;
use citrine_gb_net::{GameHash, OnlineId, Origin, RejoinPoint, SessionId};

pub(super) const KEPT_FRAMES: u32 = 20;

pub struct DropHistory {
    session: SessionId,
    round: u32,
    friend: OnlineId,
    game: GameHash,
    mine: Vec<(u32, Vec<u8>)>,
    theirs: Vec<u8>,
}

impl DropHistory {
    pub(super) fn of(
        engine: &Rollback,
        session: SessionId,
        round: u32,
        friend: OnlineId,
        game: GameHash,
    ) -> Option<Self> {
        let mine = engine.confirmed_states(true, KEPT_FRAMES as usize)?;
        let frame = mine.last()?.0;
        let (at, theirs) = engine.confirmed_states(false, 1)?.pop()?;
        if at != frame {
            return None;
        }
        Some(Self {
            session,
            round,
            friend,
            game,
            mine,
            theirs,
        })
    }

    pub(super) fn fits(&self, friend: OnlineId, game: Option<GameHash>) -> bool {
        self.friend == friend && Some(self.game) == game
    }

    pub(super) fn point(&self) -> RejoinPoint {
        RejoinPoint {
            session: self.session,
            round: self.round,
            frame: self.mine.last().map_or(0, |(at, _)| *at),
            oldest: self.mine.first().map_or(0, |(at, _)| *at),
        }
    }

    fn meet(&self, theirs: RejoinPoint) -> Option<Meeting> {
        let mine = self.point();
        if (mine.session, mine.round) != (theirs.session, theirs.round) {
            return None;
        }
        let common = mine.frame.min(theirs.frame);
        if common < mine.oldest || common < theirs.oldest {
            return None;
        }
        let my_state = match mine.frame > common {
            true => Some(
                self.mine
                    .iter()
                    .find(|(at, _)| *at == common)
                    .map(|(_, state)| state.clone())?,
            ),
            false => None,
        };
        let their_state = (theirs.frame > common).then(|| self.theirs.clone());
        Some((my_state, their_state))
    }
}

type Meeting = (Option<Vec<u8>>, Option<Vec<u8>>);

impl PlaySession {
    pub(super) fn meet_at_common_frame(&mut self) {
        let round = &mut self.round;
        let (Some(history), Some(theirs)) = (&round.rejoin, round.their_rejoin) else {
            return;
        };
        let resuming = |origin: &Option<Origin>| matches!(origin, Some(Origin::Resume(_)));
        if !resuming(&round.my_origin) || !resuming(&round.their_origin) {
            return;
        }
        let Some((mine, their)) = history.meet(theirs) else {
            return;
        };
        if let Some(state) = mine {
            round.my_origin = Some(Origin::Resume(state));
        }
        if let Some(state) = their {
            round.their_origin = Some(Origin::Resume(state));
        }
    }
}
