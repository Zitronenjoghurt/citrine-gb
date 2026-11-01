use super::*;
use crate::real_roms::{rom_of, save_from};
use citrine_gb_gamedata::Version;
use citrine_gb_net::Body;

fn real(version: Version, title: &str, save: Option<Vec<u8>>) -> Option<GameFile> {
    let (rom, _) = rom_of(version)?;
    Some(GameFile {
        save,
        ..game(&rom, title)
    })
}

fn copy(file: &GameFile) -> GameFile {
    GameFile {
        rom: file.rom.clone(),
        title: file.title.clone(),
        model: file.model,
        save: file.save.clone(),
        running: file.running,
    }
}

fn texts(side: &Side, friend: OnlineId) -> Vec<String> {
    side.online
        .conversation(friend)
        .map(|c| {
            c.entries
                .iter()
                .filter_map(|e| match &e.body {
                    Body::Text(text) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn assert_same_checks(one: &PlaySession, other: &PlaySession) -> usize {
    let want: HashMap<u32, [u64; 2]> = one
        .log()
        .unwrap()
        .checks
        .iter()
        .map(|c| (c.frame, c.hashes))
        .collect();
    let mut compared = 0;
    for check in &other.log().unwrap().checks {
        if let Some(hashes) = want.get(&check.frame) {
            assert_eq!(*hashes, check.hashes, "check of frame {}", check.frame);
            compared += 1;
        }
    }
    assert!(compared >= 3, "only {compared} checks to compare");
    compared
}

fn both_leave(pair: &mut Pair) {
    pair.host.play.leave(&mut pair.host.online);
    pair.until("the friend sees it", |p| {
        p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
    });
    assert!(pair.host.play.close().is_some());
    assert!(pair.guest.play.close().is_some());
    pair.host.own = None;
    pair.guest.own = None;
    pair.host.cable_gb = None;
    pair.guest.cable_gb = None;
}

#[test]
#[ignore]
fn two_friends_chat_play_silver_watch_and_link_gold_with_silver() {
    let save = save_from("CITRINE_SILVER_SAVE");
    let (Some(silver), Some(gold)) = (
        real(Version::Silver, "Pokémon Silver", save.clone()),
        real(Version::Gold, "Pokémon Gold", None),
    ) else {
        println!("no Silver or Gold in the ROM folder, skipped");
        return;
    };
    let started = Instant::now();
    let mut pair = Pair::new(false, 30, 10, empty());
    let (host_id, guest_id) = (pair.host.id(), pair.guest.id());
    println!("signed in and friends after {:?}", started.elapsed());

    assert!(pair.host.online.send_text(guest_id, "silver tonight?"));
    pair.until("the line arrives", |p| !texts(&p.guest, host_id).is_empty());
    assert_eq!(texts(&pair.guest, host_id), ["silver tonight?"]);
    assert!(pair.guest.online.send_text(host_id, "yes"));
    pair.until("the answer arrives", |p| {
        texts(&p.host, guest_id).len() == 2
    });
    println!("chatted after {:?}", started.elapsed());

    let session = pair.invite(copy(&silver));
    pair.accept(session, MyGame::Own(copy(&silver)));
    pair.play_frames(900);
    let (host, guest) = (pair.host.session(), pair.guest.session());
    assert_eq!(host.mode(), Some(Mode::Rollback));
    assert_eq!(guest.mode(), Some(Mode::Rollback));
    assert_same_inputs(host, guest);
    let compared = assert_same_checks(host, guest);
    assert_eq!(host.resyncs() + guest.resyncs(), 0);
    println!(
        "played Silver {} frames, {compared} checks matched, after {:?}",
        host.frame(),
        started.elapsed()
    );
    both_leave(&mut pair);

    let session = pair.invite_with(None);
    let mine = pair.guest.running(copy(&silver));
    pair.accept(session, MyGame::Own(mine));
    assert_eq!(pair.host.session().mode(), Some(Mode::Apart));
    assert!(pair.host.session_mut().hand_rom(&silver.rom));
    pair.until("the watcher sees the game", |p| {
        p.host.session().mode() == Some(Mode::Rollback)
    });
    pair.play_frames(600);
    let (watcher, player) = (pair.host.session(), pair.guest.session());
    assert!(watcher.watching_only() && !player.watching_only());
    assert!(watcher.console(true).is_none() && watcher.console(false).is_some());
    let compared = assert_same_checks(player, watcher);
    assert_eq!(watcher.resyncs() + player.resyncs(), 0);
    println!(
        "watched {compared} matching checks after {:?}",
        started.elapsed()
    );
    pair.host.play.leave(&mut pair.host.online);
    pair.until("the player sees the watcher go", |p| {
        p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
    });
    assert!(pair.host.play.close().unwrap().finish().is_none());
    let back = pair.guest.play.close().unwrap().finish().unwrap();
    assert_eq!(back.console.cartridge.rom_bytes(), silver.rom);
    pair.guest.own = None;

    let session = pair.invite(copy(&gold));
    for (side, file) in [(&mut pair.host, &gold), (&mut pair.guest, &silver)] {
        let mut gb = GameBoy::new_empty(file.model);
        gb.load_rom(&Rom::new(&file.rom)).unwrap();
        side.cable_gb = Some(gb);
    }
    pair.accept(session, MyGame::Own(copy(&silver)));
    assert_eq!(pair.host.session().mode(), Some(Mode::Cable));
    assert_eq!(pair.guest.session().mode(), Some(Mode::Cable));
    pair.until("both consoles ran", |p| {
        p.host.cable_frames >= 600 && p.guest.cable_frames >= 600
    });
    assert!(pair.host.session().cable_plugged() && pair.guest.session().cable_plugged());
    println!(
        "Gold and Silver ran on the cable after {:?}",
        started.elapsed()
    );
    both_leave(&mut pair);
    assert!(pair.host.play.session().is_none() && pair.guest.play.session().is_none());
    println!("took {:?}", started.elapsed());
}
