use super::*;
use crate::app::tabs::playing::{
    MemberAction, PlayingUi, RoomAction, RoomFacts, group_link, invite_closed, invite_note,
    member_menu, settle_room, switch_lines,
};
use crate::emulator::Emulator;
use citrine_gb_net::{Body, OLDEST_PROTOCOL_VERSION};

struct Group {
    sides: Vec<Side>,
    _relay: Relay,
    clock: Clock,
}

impl Group {
    fn new(names: &[&str], old: &[usize]) -> Self {
        let relay = Relay::start(false);
        let clock = Clock::new();
        let mut sides: Vec<Side> = names
            .iter()
            .enumerate()
            .map(|(i, _)| {
                Side::new(
                    (30, 10),
                    11 + 18 * i as u64,
                    Box::new(shelf::Homebrew),
                    &clock,
                )
            })
            .collect();
        for &i in old {
            sides[i].online.speak(OLDEST_PROTOCOL_VERSION);
        }
        let mut group = Self {
            sides,
            _relay: relay,
            clock,
        };
        let now = group.clock.now();
        for side in &mut group.sides {
            side.online.probe(
                group._relay.address.clone(),
                PASSWORD.into(),
                Trust::default(),
                now,
            );
        }
        group.until("all probed", |g| {
            g.sides.iter().all(|s| s.online.phase() == Phase::Probed)
        });
        let now = group.clock.now();
        for (side, name) in group.sides.iter_mut().zip(names) {
            side.online
                .create_invited_account((*name).into(), None, now);
        }
        group.until("all signed up", |g| {
            g.sides
                .iter()
                .all(|s| s.online.is_connected() && s.online.account().is_some())
        });
        for i in 1..group.sides.len() {
            group.befriend(0, i);
        }
        group
    }

    fn id(&self, i: usize) -> OnlineId {
        self.sides[i].id()
    }

    fn befriend(&mut self, a: usize, b: usize) {
        let (a_id, b_id) = (self.id(a), self.id(b));
        assert!(self.sides[a].online.add_friend(b_id));
        self.until("the request arrives", |g| {
            g.sides[b].online.friend(a_id).map(|f| f.state) == Some(FriendState::Incoming)
        });
        assert!(self.sides[b].online.answer(a_id, true));
        self.until("they are friends", |g| {
            g.sides[a].online.friend(b_id).map(|f| f.state) == Some(FriendState::Friend)
                && g.sides[b].online.friend(a_id).map(|f| f.state) == Some(FriendState::Friend)
        });
    }

    fn until(&mut self, what: &str, mut done: impl FnMut(&Group) -> bool) {
        let begun = self.clock.now();
        while !done(self) {
            self.clock.waited(begun, what);
            let now = self.clock.tick();
            for side in &mut self.sides {
                side.step(now);
            }
        }
    }

    fn invite(&mut self, from: usize, to: usize, game: Option<GameFile>) {
        self.invite_as(from, to, game, snake());
    }

    fn invite_as(&mut self, from: usize, to: usize, game: Option<GameFile>, theirs: GameFile) {
        let to_id = self.id(to);
        let side = &mut self.sides[from];
        assert!(side.play.invite(&mut side.online, to_id, game));
        self.until("the invite arrives", |g| {
            !g.sides[to].play.invites().is_empty()
        });
        let session = self.sides[to].play.invites()[0].session;
        let side = &mut self.sides[to];
        assert!(
            side.play
                .accept(&mut side.online, session, MyGame::Own(theirs))
        );
    }

    fn pair_of_two(names: &[&str], old: &[usize]) -> Self {
        let mut group = Self::new(names, old);
        group.invite(0, 1, Some(snake()));
        group.until("both play", |g| {
            g.sides[0].stage() == Some(Stage::Playing) && g.sides[1].stage() == Some(Stage::Playing)
        });
        group
    }

    fn room(&self, i: usize) -> &Room {
        self.sides[i].play.room().unwrap()
    }

    fn members(&self, i: usize) -> Vec<OnlineId> {
        let mut who: Vec<OnlineId> = self.room(i).others().collect();
        who.sort();
        who
    }

    fn others(&self, i: usize) -> Vec<OnlineId> {
        let mut who: Vec<OnlineId> = (0..self.sides.len())
            .filter(|j| *j != i)
            .map(|j| self.id(j))
            .collect();
        who.sort();
        who
    }

    fn frames_run(&mut self, frames: u32) {
        self.frames_run_by(0, 1, frames);
    }

    fn frames_run_by(&mut self, a: usize, b: usize, frames: u32) {
        let target = self.sides[a].frame().max(self.sides[b].frame()) + frames;
        self.until("the pair ran the frames", |g| {
            g.sides[a].frame() >= target && g.sides[b].frame() >= target
        });
    }

    fn linked(&self, a: usize, b: usize) -> bool {
        [(a, b), (b, a)].iter().all(|&(i, j)| {
            self.sides[i].stage() == Some(Stage::Playing)
                && self.sides[i].session().friend() == self.id(j)
                && self.sides[i].session().mode() == Some(Mode::Rollback)
        })
    }

    fn ask(&mut self, from: usize, to: usize) {
        let to_id = self.id(to);
        let from_id = self.id(from);
        assert!(self.sides[from].play.link(to_id));
        self.until("the request is held", |g| {
            g.room(to).links().asking().contains(&from_id)
        });
    }
}

fn three() -> Group {
    let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[]);
    group.frames_run(60);
    group.invite(0, 2, None);
    group.until("everyone sees three", |g| {
        (0..3).all(|i| g.sides[i].play.room().is_some() && g.members(i) == g.others(i))
    });
    group
}

#[cfg(feature = "gba")]
#[test]
fn a_gba_member_plays_the_game_boy_game_to_link_and_gets_their_own_back() {
    let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[]);
    group.frames_run(60);
    group.invite_as(0, 2, None, super::link_words());
    let (mia, jonas) = (group.id(1), group.id(2));
    group.until("Jonas plays his GBA game apart", |g| {
        g.sides[2].stage() == Some(Stage::Playing)
            && g.room(2)
                .pair(mia)
                .is_some_and(|p| p.friend_game().is_some())
    });
    let own = super::link_words().card().unwrap();
    assert!(group.sides[2].play.play_theirs(mia, MyGame::Own(snake())));
    assert!(group.sides[0].play.unlink());
    group.until("Mia and Jonas link in Snake", |g| g.linked(1, 2));
    let snake_card = snake().card().unwrap();
    assert_eq!(group.sides[2].session().my_game(), Some(&snake_card));
    assert_eq!(group.sides[2].play.shelved(), Some(&own));
    assert!(group.sides[2].session().shelves_mine());
    group.frames_run_by(1, 2, 120);
    assert_same_inputs(group.sides[1].session(), group.sides[2].session());
    group.sides[2].handed_back = None;
    assert!(group.sides[2].play.unlink());
    group.until("Jonas is back in his own game", |g| {
        g.room(2).links().group_of(jonas).is_none()
            && g.room(2)
                .pair(mia)
                .is_some_and(|p| p.is_apart() && p.my_game() == Some(&own))
    });
    assert!(group.sides[2].play.shelved().is_none());
    assert_eq!(group.sides[2].handed_back, None);
    group.until("Mia sees Jonas's own game again", |g| {
        g.room(1)
            .pair(jonas)
            .is_some_and(|p| p.friend_game() == Some(&own))
    });
}

#[cfg(feature = "gba")]
fn gba_group() -> Group {
    gba_group_of(&["Clara", "Mia", "Jonas"])
}

#[cfg(feature = "gba")]
fn gba_group_of(names: &[&str]) -> Group {
    let mut group = Group::new(names, &[]);
    for side in &mut group.sides {
        let rom = super::link_words().rom;
        side.own_gba = Some(Box::new(citrine_gb::gba::machine::Gba::new(&rom)));
    }
    group.invite_as(0, 1, Some(super::link_words()), super::link_words());
    group.until("Clara and Mia link", |g| g.linked(0, 1));
    group.invite_as(0, 2, None, super::link_words());
    group.until("everyone sees three", |g| {
        (0..3).all(|i| {
            g.sides[i]
                .play
                .room()
                .is_some_and(|r| r.others().count() == 2)
        })
    });
    let clara = group.id(0);
    assert!(group.sides[2].play.link(clara));
    group.until("all three run one group engine", |g| {
        (0..3).all(|i| {
            g.room(i)
                .group_engine()
                .is_some_and(|r| r.units() == [0, 1, 2])
        })
    });
    for i in 0..3 {
        let linked = group.room(i).links().group_of(clara).unwrap();
        assert_eq!(linked.members(), [clara, group.id(1), group.id(2)]);
    }
    for i in 3..names.len() {
        group.invite_as(0, i, None, super::link_words());
    }
    group.until("everyone sees everyone", |g| {
        (0..g.sides.len()).all(|i| g.sides[i].play.room().is_some() && g.members(i) == g.others(i))
    });
    group
}

#[cfg(feature = "gba")]
struct Held(Vec<u8>);

#[cfg(feature = "gba")]
impl Shelf for Held {
    fn find(&self, hash: &citrine_gb_net::GameHash) -> Option<Vec<u8>> {
        (super::sha1_of(&self.0) == Some(*hash)).then(|| self.0.clone())
    }
}

#[cfg(feature = "gba")]
#[test]
fn a_member_outside_the_group_watches_it_and_sees_the_same_frames() {
    let mut group = gba_group_of(&["Clara", "Mia", "Jonas", "Lena"]);
    group.group_runs(&[0, 1, 2], 60);
    group.sides[3]
        .play
        .set_shelf(Box::new(Held(super::link_words().rom)));
    let mia = group.id(1);
    let lena = group.id(3);
    assert!(group.sides[3].play.watch_group(mia, true));
    group.until("Lena sees Mia's screen", |g| {
        g.room(3).watched_console().is_some()
    });
    group.until("Clara lists Lena as a watcher", |g| {
        g.room(0)
            .links()
            .group_of(g.id(0))
            .is_some_and(|l| l.watchers().contains(&lena))
    });
    let start = group.room(3).watch_engine().unwrap().frame();
    group.until("Lena follows 180 frames", |g| {
        g.room(3)
            .watch_engine()
            .is_some_and(|r| r.frame() >= start + 180)
    });
    let theirs: HashMap<u32, Vec<u64>> = group
        .room(1)
        .group_engine()
        .unwrap()
        .log
        .group_checks
        .iter()
        .cloned()
        .collect();
    let mut compared = 0;
    for (frame, hashes) in &group.room(3).watch_engine().unwrap().log.group_checks {
        if let Some(mia_saw) = theirs.get(frame) {
            assert_eq!(hashes[..3], mia_saw[..3], "check of frame {frame}");
            compared += 1;
        }
    }
    assert!(compared >= 2, "{compared} checks compared");
    assert_eq!(group.room(3).watching(), Some(mia));
    assert!(group.sides[3].play.watch_group(mia, false));
    group.until("Clara drops Lena from the watchers", |g| {
        g.room(0)
            .links()
            .group_of(g.id(0))
            .is_some_and(|l| !l.watchers().contains(&lena))
    });
}

#[cfg(feature = "gba")]
impl Group {
    fn group_frame(&self, i: usize) -> u32 {
        self.room(i).group_engine().map_or(0, |r| r.frame())
    }

    fn group_runs(&mut self, sides: &[usize], frames: u32) {
        let target = sides
            .iter()
            .map(|&i| self.group_frame(i))
            .max()
            .unwrap_or(0)
            + frames;
        self.until("the group ran the frames", |g| {
            sides.iter().all(|&i| g.group_frame(i) >= target)
        });
    }
}

#[cfg(feature = "gba")]
#[test]
fn the_group_engine_draws_the_partner_screens_a_member_watches() {
    let mut group = gba_group();
    group.group_runs(&[0, 1, 2], 30);
    for i in 0..3 {
        let session = group.sides[i].session();
        assert!(session.sees_friend(), "member {i} sees a partner");
        assert!(
            group.room(i).group_engine().unwrap().watches(),
            "member {i}"
        );
    }
    group.sides[0].session_mut().set_watch_friend(false);
    assert!(!group.room(0).group_engine().unwrap().watches());
}

#[cfg(feature = "gba")]
#[test]
fn a_group_member_names_every_partner_and_the_slowest_link() {
    let mut group = gba_group();
    group.group_runs(&[0, 1, 2], 30);
    let side = &group.sides[1];
    let (partners, slowest) = group_link(&side.play, &side.online).unwrap();
    assert_eq!(partners, "Clara and Jonas");
    let facts = RoomFacts::of(&side.play, &side.online, 0).unwrap();
    assert_eq!(slowest, facts.partners().iter().filter_map(|p| p.ms).max());
}

#[cfg(feature = "gba")]
#[test]
fn three_gba_members_link_into_one_group_and_play_one_game() {
    let mut group = gba_group();
    group.until("the group ran 240 frames", |g| {
        (0..3).all(|i| g.room(i).group_engine().is_some_and(|r| r.frame() >= 240))
    });
    let logs: Vec<engine::Log> = (0..3)
        .map(|i| group.room(i).group_engine().unwrap().log.clone())
        .collect();
    for unit in 0..3 {
        for frame in 0..240 {
            let inputs: Vec<_> = logs
                .iter()
                .filter_map(|log| log.units[unit].get(frame).cloned().flatten())
                .collect();
            assert!(
                inputs.windows(2).all(|w| w[0] == w[1]),
                "unit {unit} frame {frame}"
            );
        }
    }
    let checks: Vec<HashMap<u32, Vec<u64>>> = logs
        .iter()
        .map(|log| log.group_checks.iter().cloned().collect())
        .collect();
    let mut compared = 0;
    for (frame, hashes) in &checks[0] {
        for other in &checks[1..] {
            if let Some(theirs) = other.get(frame) {
                assert_eq!(hashes, theirs, "check of frame {frame}");
                compared += 1;
            }
        }
    }
    assert!(compared > 0);
}

#[cfg(feature = "gba")]
#[test]
fn a_member_who_unlinks_gets_their_game_back_and_the_rest_play_on() {
    let mut group = gba_group();
    group.group_runs(&[0, 1, 2], 60);
    let jonas = group.id(2);
    assert!(group.sides[2].play.unlink());
    assert!(group.room(2).group_engine().is_none());
    assert!(!group.sides[2].session().in_group());
    group.until("Clara and Mia see Jonas unlinked", |g| {
        (0..2).all(|i| {
            g.room(i)
                .links()
                .group_of(g.id(0))
                .is_some_and(|l| !l.holds(jonas))
        })
    });
    group.group_runs(&[0, 1], 120);
}

#[cfg(feature = "gba")]
#[test]
fn a_member_who_stops_answering_can_be_unlinked_by_anyone() {
    let mut group = gba_group();
    group.group_runs(&[0, 1, 2], 60);
    let jonas = group.id(2);
    group.sides[2].down.set(true);
    group.until("Clara waits for Jonas", |g| {
        g.room(0)
            .group_waiting_for(g.clock.now())
            .is_some_and(|(who, _)| who == jonas)
    });
    assert!(group.sides[1].play.unlink_member(jonas));
    group.until("Clara sees Jonas unlinked", |g| {
        g.room(0)
            .links()
            .group_of(g.id(0))
            .is_some_and(|l| !l.holds(jonas))
    });
    group.group_runs(&[0, 1], 120);
}

#[cfg(feature = "gba")]
#[test]
fn a_member_who_pauses_shows_as_paused_to_the_group() {
    let mut group = gba_group();
    let mia = group.id(1);
    group.sides[1].play.set_status(true, false);
    group.until("Clara sees Mia paused", |g| {
        g.room(0).paused_members().contains(&mia)
    });
    group.sides[1].play.set_status(false, true);
    group.until("Clara sees Mia away", |g| {
        !g.room(0).paused_members().contains(&mia) && g.room(0).away_members().contains(&mia)
    });
}

#[test]
fn a_third_member_joins_while_the_pair_keeps_playing() {
    let mut group = three();
    let round = group.sides[0].session().round();
    let jonas = group.id(2);
    group.until("Jonas plays apart with Clara", |g| {
        g.sides[2].stage() == Some(Stage::Playing)
    });
    assert_eq!(group.sides[2].session().friend(), group.id(0));
    assert_eq!(group.sides[2].session().mode(), Some(Mode::Apart));
    assert_eq!(group.room(2).member(group.id(1)).unwrap().name, "Mia");
    assert_eq!(group.room(1).member(jonas).unwrap().name, "Jonas");
    group.frames_run(120);
    assert_eq!(group.sides[0].session().round(), round);
    assert_eq!(group.sides[1].session().round(), round);
    assert_eq!(group.sides[0].session().friend(), group.id(1));
    assert_eq!(group.sides[0].session().mode(), Some(Mode::Rollback));
    assert_same_inputs(group.sides[0].session(), group.sides[1].session());
}

#[test]
fn a_game_change_reaches_every_member() {
    let mut group = three();
    let clara = group.id(0);
    group.until("Jonas plays apart", |g| {
        g.sides[2].stage() == Some(Stage::Playing)
    });
    let card = super::custom().card().unwrap();
    assert!(group.sides[0].play.bring(MyGame::Own(super::custom())));
    group.until("Mia and Jonas see the new game", |g| {
        (1..3).all(|i| {
            g.room(i)
                .pair(clara)
                .is_some_and(|p| p.friend_game() == Some(&card))
        })
    });
}

#[test]
fn joining_from_a_second_device_ends_the_room_on_the_first() {
    let mut group = three();
    let room = group.room(2).id().unwrap();
    let jonas = group.id(2);
    let side = &mut group.sides[2];
    side.play.receive(
        &mut side.online,
        ServerMsg::MemberGone {
            room,
            who: jonas,
            reason: EndReason::Left,
        },
        group.clock.now(),
    );
    assert_eq!(group.sides[2].stage(), Some(Stage::Ended(Ending::Moved)));
    assert!(!group.sides[2].play.is_live());
}

#[test]
fn hiding_my_screen_hides_it_from_everyone_in_the_room() {
    let mut group = three();
    let clara = group.id(0);
    group.until("both see Clara's screen", |g| {
        [1, 2].iter().all(|&i| {
            g.room(i)
                .pair(clara)
                .is_some_and(|p| p.friend_lets_me_watch())
        })
    });
    group.sides[0].play.set_let_watch(false);
    group.until("nobody sees Clara's screen", |g| {
        [1, 2].iter().all(|&i| {
            g.room(i)
                .pair(clara)
                .is_some_and(|p| !p.friend_lets_me_watch())
        })
    });
    let room = group.room(0);
    assert!(
        room.others()
            .all(|w| !room.pair(w).unwrap().lets_friend_watch())
    );
}

#[test]
fn an_invite_from_another_room_asks_to_leave_this_one_first() {
    let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas", "Ana"], &[]);
    group.frames_run(60);
    group.invite(0, 2, None);
    let clara = group.id(0);
    group.until("everyone sees three", |g| {
        (0..3).all(|i| {
            g.sides[i]
                .play
                .room()
                .is_some_and(|r| r.members().len() == 2)
        })
    });
    let side = &mut group.sides[3];
    assert!(side.play.invite(&mut side.online, clara, Some(snake())));
    group.until("Ana's invite arrives", |g| {
        !g.sides[0].play.invites().is_empty()
    });
    let side = &mut group.sides[0];
    let invite = side.play.invites()[0].clone();
    assert!(
        !side
            .play
            .accept(&mut side.online, invite.session, MyGame::Own(snake()))
    );
    let lines = switch_lines(&side.play, &side.online, "Ana").unwrap();
    assert_eq!(lines.0, "Switch to Ana?");
    assert!(
        lines.1 == "Leave the room with Mia and Jonas?"
            || lines.1 == "Leave the room with Jonas and Mia?",
        "{}",
        lines.1
    );
    side.play.leave(&mut side.online);
    assert!(!side.play.is_live());
    side.play.close();
    assert!(
        side.play
            .accept(&mut side.online, invite.session, MyGame::Own(snake()))
    );
    group.until("Clara plays with Ana", |g| {
        g.sides[0].stage() == Some(Stage::Playing) && g.sides[0].session().friend() == g.id(3)
    });
    group.until("Mia and Jonas stay together", |g| {
        [1, 2]
            .iter()
            .all(|&i| g.room(i).members().len() == 1 && g.sides[i].play.is_live())
    });
}

#[test]
fn a_member_who_is_not_a_friend_can_be_added_from_the_room() {
    let mut group = three();
    let (mia, jonas) = (group.id(1), group.id(2));
    let side = &group.sides[2];
    let facts = RoomFacts::of(&side.play, &side.online, 0).unwrap();
    let person = facts.person(mia).unwrap();
    assert_eq!((person.name.as_str(), person.friend), ("Mia", None));
    let menu = member_menu(&facts, mia);
    assert!(menu.add_friend && !menu.message && !menu.profile);
    let side = &mut group.sides[2];
    let go = settle_room(
        RoomAction::Member(mia, MemberAction::AddFriend),
        &mut side.play,
        &mut side.online,
        &Emulator::default(),
        &mut PlayingUi::default(),
    );
    assert!(go.is_none());
    group.until("Mia gets Jonas's request", |g| {
        g.sides[1].online.friend(jonas).map(|f| f.state) == Some(FriendState::Incoming)
    });
}

#[test]
fn watching_a_member_whose_game_i_lack_asks_for_my_copy() {
    let mut group = Group::new(&["Clara", "Mia", "Jonas"], &[]);
    group.invite_as(0, 1, Some(snake()), super::custom());
    group.until("both play", |g| {
        (0..2).all(|i| g.sides[i].stage() == Some(Stage::Playing))
    });
    group.invite(0, 2, None);
    let mia = group.id(1);
    group.until("Clara sees the room", |g| g.room(0).members().len() == 2);
    let side = &group.sides[0];
    let facts = RoomFacts::of(&side.play, &side.online, 0).unwrap();
    let watch = member_menu(&facts, mia).watch.unwrap();
    assert!(watch.missing && watch.copy);
    assert_eq!(watch.why_off.as_deref(), Some("Needs My Snake"));
    let side = &mut group.sides[0];
    let mut ui = PlayingUi::default();
    settle_room(
        RoomAction::Member(mia, MemberAction::OpenCopy),
        &mut side.play,
        &mut side.online,
        &Emulator::default(),
        &mut ui,
    );
    assert!(ui.wants_copy);
}

#[test]
fn room_chat_reaches_every_member() {
    let mut group = three();
    let jonas_side = &mut group.sides[2];
    assert!(
        jonas_side
            .play
            .say(&mut jonas_side.online, "hello room".into())
    );
    let jonas = group.id(2);
    group.until("Clara and Mia read it", |g| {
        [0, 1].iter().all(|&i| {
            g.room(i).chat().iter().any(|line| {
                line.from == jonas && line.envelope.body == Body::Text("hello room".into())
            })
        })
    });
    assert_eq!(group.room(2).chat().len(), 1);
}

#[test]
fn a_dropped_member_rejoins_and_the_pair_plays_on() {
    let mut group = Group::pair_of_two(&["Clara", "Mia"], &[]);
    group.frames_run(60);
    let round = group.sides[0].session().round();
    let mia = group.id(1);
    group.sides[1].cut.set(true);
    group.until("Clara sees Mia reconnecting", |g| {
        g.room(0).member(mia).is_some_and(|m| m.state.reconnecting)
    });
    assert_eq!(group.sides[0].stage(), Some(Stage::Playing));
    group.until("Mia is back", |g| {
        !g.room(1).rejoining() && g.room(0).member(mia).is_some_and(|m| !m.state.reconnecting)
    });
    group.until("both play a new round", |g| {
        (0..2).all(|i| {
            g.sides[i].stage() == Some(Stage::Playing) && g.sides[i].session().round() > round
        })
    });
    group.frames_run(60);
    assert_same_inputs(group.sides[0].session(), group.sides[1].session());
}

#[test]
fn a_pair_dropped_in_a_room_of_three_hands_back_a_paused_console() {
    let mut group = three();
    group.until("Clara and Mia play linked", |g| g.linked(0, 1));
    group.frames_run(60);
    let mia = group.id(1);
    group.sides[0].handed_back = None;
    group.sides[1].down.set(true);
    group.until("Clara gets her console back", |g| {
        g.sides[0].handed_back.is_some_and(|(_, paused)| paused)
    });
    assert_eq!(group.sides[0].handed_back, Some((mia, true)));
    assert!(group.sides[0].play.is_live());
}

#[test]
fn an_old_partner_still_plays_a_pair() {
    let mut group = Group::pair_of_two(&["Clara", "Mia"], &[1]);
    group.frames_run(120);
    assert_eq!(group.sides[0].session().mode(), Some(Mode::Rollback));
    assert_same_inputs(group.sides[0].session(), group.sides[1].session());
    let mia = &mut group.sides[1];
    mia.play.leave(&mut mia.online);
    group.until("Clara sees Mia leave", |g| {
        g.sides[0].stage() == Some(Stage::Ended(Ending::FriendLeft))
    });
}

#[test]
fn a_link_request_waits_until_the_pair_unlinks() {
    let mut group = three();
    group.until("Jonas plays apart", |g| {
        g.sides[2].stage() == Some(Stage::Playing)
    });
    group.ask(2, 1);
    group.frames_run(60);
    assert!(group.linked(0, 1));
    let (mia, jonas) = (group.id(1), group.id(2));
    assert!(group.sides[0].play.unlink());
    group.until("Mia and Jonas link", |g| g.linked(1, 2));
    for i in 0..3 {
        let room = group.room(i);
        assert_eq!(room.links().groups().len(), 1);
        assert_eq!(room.links().group_of(jonas).unwrap().authority(), mia);
    }
    assert_eq!(group.sides[1].session().role(), Role::Host);
    assert_eq!(group.sides[2].session().role(), Role::Guest);
    assert!(group.room(0).pair(mia).unwrap().is_apart());
    group.frames_run_by(1, 2, 120);
    assert_same_inputs(group.sides[1].session(), group.sides[2].session());
}

#[test]
fn the_next_in_line_links_when_the_authority_leaves() {
    let mut group = three();
    group.until("Jonas plays apart", |g| {
        g.sides[2].stage() == Some(Stage::Playing)
    });
    group.ask(2, 1);
    let clara = group.id(0);
    let mia = group.id(1);
    let side = &mut group.sides[0];
    side.play.leave(&mut side.online);
    group.until("Mia and Jonas link", |g| g.linked(1, 2));
    assert!(group.room(1).member(clara).is_none());
    assert_eq!(
        group.room(2).links().group_of(mia).unwrap().authority(),
        mia
    );
    group.frames_run_by(1, 2, 120);
    assert_same_inputs(group.sides[1].session(), group.sides[2].session());
}

#[test]
fn taking_a_waiting_request_unlinks_and_links_with_the_asker() {
    let mut group = three();
    group.until("Jonas plays apart", |g| {
        g.sides[2].stage() == Some(Stage::Playing)
    });
    group.ask(2, 1);
    let jonas = group.id(2);
    assert!(group.sides[1].play.take_link(jonas));
    group.until("Mia and Jonas link", |g| g.linked(1, 2));
    assert!(group.room(1).links().asking().is_empty());
    assert!(group.room(0).links().group_of(group.id(0)).is_none());
}

#[test]
fn an_invite_an_old_partner_blocks_names_the_friend_it_was_for() {
    let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[1]);
    group.frames_run(60);
    let jonas = group.id(2);
    let side = &mut group.sides[0];
    assert!(side.play.invite(&mut side.online, jonas, None));
    group.until("the relay answers", |g| g.room(0).invite_error().is_some());
    assert_eq!(group.room(0).invite_error(), Some(ErrorCode::NeedsUpdate));
    assert_eq!(group.room(0).invite_failed(), Some(jonas));
    assert!(group.room(0).inviting().is_empty());
}

#[test]
fn a_room_of_two_keeps_the_pair_bar_and_three_get_the_room_bar() {
    let pair = Group::pair_of_two(&["Clara", "Mia"], &[]);
    assert!(RoomFacts::of(&pair.sides[0].play, &pair.sides[0].online, 0).is_none());
    let group = three();
    let facts = RoomFacts::of(&group.sides[0].play, &group.sides[0].online, 0).unwrap();
    assert_eq!(facts.people.len(), 3);
}

#[test]
fn an_old_member_closes_the_invite_before_anyone_tries() {
    let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[1]);
    let (mia, jonas) = (group.id(1), group.id(2));
    group.until("Clara knows Mia's client is old", |g| {
        g.room(0).old_client() == Some(mia)
    });
    assert_eq!(group.room(0).invite_error(), None);
    let side = &group.sides[0];
    assert_eq!(
        invite_note(&side.play, &side.online, jonas).as_deref(),
        Some("Can't invite until Mia updates Citrine")
    );
    assert!(invite_closed(&side.play, &side.online, jonas));
}

#[test]
fn a_full_room_greys_play_together_before_anyone_tries() {
    let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[]);
    let jonas = group.id(2);
    let side = &group.sides[0];
    assert_eq!(invite_note(&side.play, &side.online, jonas), None);
    let me = side.id();
    let mut list = group.room(0).members().to_vec();
    let template = list[0].clone();
    while list.len() + 1 < citrine_gb_net::ROOM_MEMBERS {
        list.push(RoomMember {
            who: OnlineId::new(1000 + list.len() as u64).unwrap(),
            ..template.clone()
        });
    }
    let members: Vec<citrine_gb_net::Member> = list
        .into_iter()
        .map(|m| citrine_gb_net::Member {
            who: m.who,
            display_name: m.name,
            public_key: m.key,
            state: m.state,
        })
        .collect();
    let side = &mut group.sides[0];
    let room = side.play.room.as_mut().unwrap();
    room.joined(me, members, false, group.clock.now());
    let side = &group.sides[0];
    assert_eq!(
        invite_note(&side.play, &side.online, jonas).as_deref(),
        Some("Room is full")
    );
    assert!(invite_closed(&side.play, &side.online, jonas));
}

#[test]
fn a_blocked_invite_says_why_on_the_friend_it_was_for() {
    let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[1]);
    group.frames_run(60);
    let jonas = group.id(2);
    let side = &mut group.sides[0];
    assert!(side.play.invite(&mut side.online, jonas, None));
    group.until("the relay answers", |g| g.room(0).invite_error().is_some());
    assert_eq!(
        invite_note(&group.sides[0].play, &group.sides[0].online, jonas).as_deref(),
        Some("Can't invite until Mia updates Citrine")
    );
    assert!(invite_closed(
        &group.sides[0].play,
        &group.sides[0].online,
        jonas
    ));
}

#[cfg(feature = "gba")]
#[test]
fn a_watcher_outside_the_friend_list_sees_the_watched_members_name() {
    let mut group = gba_group_of(&["Clara", "Mia", "Jonas", "Lena"]);
    group.group_runs(&[0, 1, 2], 30);
    group.sides[3]
        .play
        .set_shelf(Box::new(Held(super::link_words().rom)));
    let mia = group.id(1);
    assert!(group.sides[3].online.friend(mia).is_none());
    assert!(group.sides[3].play.watch_group(mia, true));
    group.until("Lena sees Mia's screen", |g| {
        g.room(3).watched_console().is_some()
    });
    let lena = &group.sides[3];
    let session = lena.play.session().expect("Lena's session");
    assert_eq!(
        crate::app::tabs::playing::watched_name(&lena.play, &lena.online, session),
        "Mia"
    );
}
