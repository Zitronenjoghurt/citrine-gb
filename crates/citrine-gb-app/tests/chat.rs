use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::recording::{Button, Recording};
use citrine_gb::rom::Rom;
use citrine_gb_app::online::chat::{Change, Delivery, Failure, Fetch, NewShare};
use citrine_gb_app::online::chat_media::{decode_image, pack_screenshot, unpack_screenshot};
use citrine_gb_app::online::{Online, OnlineSettings, Phase, ServerAddress, Trust};
use citrine_gb_net::{Body, ClientMsg, Envelope, ErrorCode, FileId, OnlineId, ShareKind};
use citrine_gb_server::{Config, Running, start};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use web_time::Instant;

const PASSWORD: &str = "lemon tree";
const DAY: u64 = 86_400;
const LETTER: &str = "Hey! I finally beat the second castle last night, it took me about forty tries. \
The trick is to wait on the moving platform until the fireballs pass, then jump twice \
and hold right. I recorded the run so you can see how it goes, but my thumbs were \
shaking at the end so the last jump looks a bit silly. Are you still stuck on the ice \
level? If you want I can send you my save file so you can skip it, or we could play \
together on Saturday after lunch. My brother says the secret room behind the waterfall \
has an extra life, but I never found the switch for it. Also the new music in world \
three is so good, I keep humming it at work and people look at me funny. Let me know \
what time works for you, I am free all afternoon. Bring snacks, I will make lemonade. \
See you soon, and good luck with the boss, he is easier than he looks once you learn \
the pattern of his attacks. PS: the cat walked across the keyboard while I typed \
this, so sorry about any typos.";

struct Relay {
    runtime: tokio::runtime::Runtime,
    running: Option<Running>,
    address: ServerAddress,
    dir: tempfile::TempDir,
}

impl Relay {
    fn start(storage_mb: u64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let vars: HashMap<&str, String> = HashMap::from([
            ("CITRINE_BIND", "127.0.0.1".into()),
            ("CITRINE_PORT", "0".into()),
            ("CITRINE_DISK_FLOOR_MB", "0".into()),
            ("CITRINE_DISK_FLOOR_PERCENT", "0".into()),
            (
                "CITRINE_DATA_DIR",
                dir.path().join("data").display().to_string(),
            ),
            ("CITRINE_WEB", "false".into()),
            ("CITRINE_RELAY", "true".into()),
            ("CITRINE_RELAY_PASSWORD", PASSWORD.into()),
            ("CITRINE_STORAGE_MB", storage_mb.to_string()),
            ("CITRINE_MESSAGES_PER_SECOND", "100000".into()),
            ("CITRINE_MESSAGE_BURST", "100000".into()),
            ("CITRINE_CHATS_PER_SECOND", "100000".into()),
            ("CITRINE_CHAT_BURST", "100000".into()),
            ("CITRINE_CHAT_KB_PER_SECOND", "100000".into()),
        ]);
        let config = Config::from_vars(|k| vars.get(k).cloned()).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let running = runtime.block_on(start(config)).unwrap();
        let address = ServerAddress::parse(&format!("127.0.0.1:{}", running.addr.port())).unwrap();
        Self {
            runtime,
            running: Some(running),
            address,
            dir,
        }
    }

    fn data(&self) -> std::path::PathBuf {
        self.dir.path().join("data")
    }

    fn tidy(&self, now: u64) {
        let running = self.running.as_ref().unwrap();
        self.runtime.block_on(running.tidy(now));
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            self.runtime.block_on(running.stop());
        }
    }
}

fn wait(people: &mut [&mut Online], what: &str, done: impl Fn(&[&mut Online]) -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        for person in people.iter_mut() {
            person.update(Instant::now());
        }
        if done(people) {
            return;
        }
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn settle(people: &mut [&mut Online], for_ms: u64) {
    let until = Instant::now() + Duration::from_millis(for_ms);
    while Instant::now() < until {
        for person in people.iter_mut() {
            person.update(Instant::now());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn join(relay: &Relay, name: &str) -> (Online, OnlineSettings, OnlineId) {
    let mut online = Online::default();
    online.probe(
        relay.address.clone(),
        PASSWORD.into(),
        Trust::default(),
        Instant::now(),
    );
    wait(&mut [&mut online], "the server's info", |p| {
        p[0].phase() == Phase::Probed
    });
    online.create_account(name.into(), Instant::now());
    wait(&mut [&mut online], "the welcome", |p| p[0].is_connected());
    let joined = online.take_joined().unwrap();
    let id = joined.account.online_id;
    let mut settings = OnlineSettings::default();
    settings.remember(
        &joined.address,
        &joined.relay_id,
        &joined.password,
        &joined.account,
        &joined.trust,
    );
    (online, settings, id)
}

fn befriend(a: &mut Online, b: &mut Online, a_id: OnlineId, b_id: OnlineId) {
    a.add_friend(b_id);
    wait(&mut [&mut *a, &mut *b], "the request", |p| {
        p[1].friend(a_id).is_some()
    });
    b.answer(a_id, true);
    wait(&mut [&mut *a, &mut *b], "the friendship", |p| {
        p[0].can_chat(b_id) && p[1].can_chat(a_id)
    });
}

fn texts(online: &Online, friend: OnlineId) -> Vec<String> {
    online
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

fn last_delivery(online: &Online, friend: OnlineId) -> Option<Delivery> {
    online
        .conversation(friend)?
        .entries
        .last()
        .map(|e| e.delivery)
}

fn on_relay(delivery: Delivery) -> bool {
    matches!(
        delivery,
        Delivery::Sent | Delivery::Delivered(_) | Delivery::Read(_)
    )
}

fn last_share(online: &Online, friend: OnlineId) -> Option<(u64, citrine_gb_net::Share)> {
    let entry = online.conversation(friend)?.entries.last()?;
    match &entry.body {
        Body::Share(share) => Some((entry.id, share.clone())),
        _ => None,
    }
}

fn received_shares(online: &Online, friend: OnlineId) -> usize {
    online.conversation(friend).map_or(0, |c| {
        c.entries
            .iter()
            .filter(|e| !e.mine && matches!(e.body, Body::Share(_)))
            .count()
    })
}

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

fn screenshot_png() -> (Vec<u8>, Vec<u8>) {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&Rom::new(include_bytes!("../homebrew/libbet.gb")))
        .unwrap();
    for _ in 0..240 {
        gb.run_frame();
    }
    let native = gb.frame().as_slice().to_vec();
    let scale = 3;
    let (width, height) = (160 * scale, 144 * scale);
    let mut rgba = vec![0; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let from = ((y / scale) * 160 + x / scale) * 4;
            let to = (y * width + x) * 4;
            rgba[to..to + 4].copy_from_slice(&native[from..from + 4]);
        }
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&rgba).unwrap();
    }
    (png, rgba)
}

fn replay_json() -> Vec<u8> {
    let mut recording = Recording::new("ab".repeat(32), GbModel::Dmg);
    let buttons = [
        Button::Right,
        Button::A,
        Button::Left,
        Button::B,
        Button::Start,
    ];
    for i in 0..600u64 {
        let button = buttons[(i as usize / 2) % buttons.len()];
        recording.push(i * 70_224 * 3, button, i % 2 == 0);
    }
    recording.to_json().unwrap().into_bytes()
}

fn share(kind: ShareKind, data: Vec<u8>, width: u32, height: u32, preview: Vec<u8>) -> NewShare {
    NewShare {
        kind,
        data,
        game: "LIBBET".into(),
        rom_sha256: Some("ab".repeat(32)),
        created_at: 1_761_800_000,
        width,
        height,
        length_ms: (kind != ShareKind::Screenshot).then_some(10_000),
        preview,
    }
}

fn fetch(ben: &mut Online, ana: &mut Online, ana_id: OnlineId, file: FileId) -> Vec<u8> {
    ben.fetch(ana_id, file);
    wait(&mut [&mut *ben, &mut *ana], "the download", |p| {
        matches!(p[0].fetched(file), Some(Fetch::Ready(_) | Fetch::Failed(_)))
    });
    match ben.take_fetched(file) {
        Some(Fetch::Ready(bytes)) => bytes,
        other => panic!("download failed: {other:?}"),
    }
}

fn send_and_receive(
    ana: &mut Online,
    ben: &mut Online,
    ana_id: OnlineId,
    ben_id: OnlineId,
    new: NewShare,
) -> (citrine_gb_net::Share, Vec<u8>) {
    let before = received_shares(ben, ana_id);
    assert!(ana.share(ben_id, new));
    wait(&mut [&mut *ana, &mut *ben], "the share", |p| {
        p[0].conversation(ben_id)
            .and_then(|c| c.entries.last())
            .is_some_and(|e| on_relay(e.delivery))
            && received_shares(p[1], ana_id) == before + 1
    });
    let (_, share) = last_share(ben, ana_id).unwrap();
    let bytes = fetch(ben, ana, ana_id, share.file);
    (share, bytes)
}

fn files_in(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files_in(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn bytes_in(dir: &Path) -> u64 {
    files_in(dir)
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn a_fresh_start_keeps_the_messages_that_waited_for_it() {
    let relay = Relay::start(1);
    let (mut ana, _, ana_id) = join(&relay, "Ana");
    let (mut ben, ben_settings, ben_id) = join(&relay, "Ben");
    befriend(&mut ana, &mut ben, ana_id, ben_id);
    ben.leave();
    wait(&mut [&mut ana, &mut ben], "Ben offline", |p| {
        !p[1].is_connected()
    });
    drop(ben);
    for text in ["one", "two", "three"] {
        assert!(ana.send_text(ben_id, text));
    }
    wait(&mut [&mut ana], "the stored messages", |p| {
        p[0].conversation(ben_id)
            .unwrap()
            .entries
            .iter()
            .all(|e| e.delivery == Delivery::Sent)
    });
    let mut fresh = Online::default();
    fresh.sign_in(ben_settings.joined_relay().unwrap(), Instant::now());
    wait(&mut [&mut fresh], "the sign in", |p| {
        p[0].phase() == Phase::SigningIn
    });
    std::thread::sleep(Duration::from_millis(500));
    fresh.update(Instant::now());
    assert!(fresh.is_connected());
    assert_eq!(texts(&fresh, ana_id), ["one", "two", "three"]);
}

fn entry_of(online: &Online, friend: OnlineId, id: u64) -> citrine_gb_app::online::chat::ChatEntry {
    online
        .conversation(friend)
        .and_then(|c| c.entries.iter().find(|e| e.id == id))
        .cloned()
        .expect("the message")
}

#[test]
fn an_edit_and_a_delete_reach_the_friend() {
    let relay = Relay::start(1);
    let (mut ana, _, ana_id) = join(&relay, "Ana");
    let (mut ben, _, ben_id) = join(&relay, "Ben");
    befriend(&mut ana, &mut ben, ana_id, ben_id);

    assert!(ana.send_text(ben_id, "See you at 3"));
    assert!(ana.send_text(ben_id, "Bring the cable"));
    wait(&mut [&mut ana, &mut ben], "both messages", |p| {
        texts(p[1], ana_id).len() == 2
    });
    let ids: Vec<u64> = ana
        .conversation(ben_id)
        .unwrap()
        .entries
        .iter()
        .map(|e| e.id)
        .collect();
    assert!(!ben.can_edit(ana_id, ids[0]));

    assert!(ana.edit_text(ben_id, ids[0], "See you at 4"));
    wait(&mut [&mut ana, &mut ben], "the edit", |p| {
        texts(p[1], ana_id) == ["See you at 4", "Bring the cable"]
    });
    assert!(entry_of(&ben, ana_id, ids[0]).is_edited());
    wait(&mut [&mut ana], "the edit on the relay", |p| {
        entry_of(p[0], ben_id, ids[0]).change == Change::Edited { told: true }
    });

    ana.delete_message(ben_id, ids[1]);
    wait(&mut [&mut ana, &mut ben], "the delete", |p| {
        entry_of(p[1], ana_id, ids[1]).is_deleted()
    });
    let gone = entry_of(&ben, ana_id, ids[1]);
    assert_eq!(gone.body, Body::Text(String::new()));
    assert_eq!(texts(&ben, ana_id), ["See you at 4", ""]);
    assert_eq!(ben.unread(ana_id), 1);
    wait(&mut [&mut ana], "the delete on the relay", |p| {
        entry_of(p[0], ben_id, ids[1]).change == Change::Deleted { told: true }
    });

    assert!(ben.send_text(ana_id, "ok"));
    wait(&mut [&mut ana, &mut ben], "Ben's reply", |p| {
        texts(p[0], ben_id).len() == 3
    });
    let theirs = ben.conversation(ana_id).unwrap().entries[2].id;
    assert!(!ana.can_edit(ben_id, theirs));
    assert!(!ana.edit_text(ben_id, theirs, "hacked"));
    ana.delete_message(ben_id, theirs);
    assert!(!entry_of(&ana, ben_id, theirs).is_deleted());
}

#[test]
fn a_friend_cannot_change_my_messages() {
    let relay = Relay::start(1);
    let (mut ana, _, ana_id) = join(&relay, "Ana");
    let (mut ben, ben_settings, ben_id) = join(&relay, "Ben");
    befriend(&mut ana, &mut ben, ana_id, ben_id);
    assert!(ana.send_text(ben_id, "mine"));
    wait(&mut [&mut ana, &mut ben], "the message", |p| {
        texts(p[1], ana_id).len() == 1
    });
    let id = ana.conversation(ben_id).unwrap().entries[0].id;
    let keys = ben_settings.joined_relay().unwrap().relay_id.keys();
    let ana_key = ben.friend(ana_id).unwrap().public_key;
    let forged = [
        Body::Edit(citrine_gb_net::Edit {
            id,
            text: "not mine".into(),
        }),
        Body::Delete(id),
    ];
    for body in forged {
        let sealed = keys.seal_envelope(&ana_key, &Envelope::new(body));
        assert!(ben.send(ClientMsg::Chat {
            tag: 9000,
            to: ana_id,
            sealed,
        }));
    }
    assert!(ben.send_text(ana_id, "after"));
    wait(&mut [&mut ana, &mut ben], "the later message", |p| {
        texts(p[0], ben_id) == ["mine", "after"]
    });
    let kept = entry_of(&ana, ben_id, id);
    assert_eq!(kept.change, Change::Kept);
}

#[test]
fn an_edit_made_offline_goes_once_back_online() {
    let relay = Relay::start(1);
    let (mut ana, ana_settings, ana_id) = join(&relay, "Ana");
    let (mut ben, _, ben_id) = join(&relay, "Ben");
    befriend(&mut ana, &mut ben, ana_id, ben_id);
    assert!(ana.send_text(ben_id, "typo hre"));
    wait(&mut [&mut ana, &mut ben], "the message on the relay", |p| {
        texts(p[1], ana_id).len() == 1 && last_delivery(p[0], ben_id).is_some_and(on_relay)
    });
    let id = ana.conversation(ben_id).unwrap().entries[0].id;
    ana.leave();
    wait(&mut [&mut ana], "Ana offline", |p| !p[0].is_connected());
    assert!(ana.edit_text(ben_id, id, "typo here"));
    assert_eq!(
        entry_of(&ana, ben_id, id).change,
        Change::Edited { told: false }
    );
    ana.sign_in(ana_settings.joined_relay().unwrap(), Instant::now());
    wait(&mut [&mut ana, &mut ben], "the edit after sign in", |p| {
        texts(p[1], ana_id) == ["typo here"]
    });
}

#[test]
fn a_shared_palette_arrives_intact() {
    let relay = Relay::start(1);
    let (mut ana, _, ana_id) = join(&relay, "Ana");
    let (mut ben, _, ben_id) = join(&relay, "Ben");
    befriend(&mut ana, &mut ben, ana_id, ben_id);
    let palette = r#"{"name":"Lemon","colors":[[255,255,255],[204,204,0],[136,136,0],[34,34,0]]}"#;
    assert!(ana.send_palette(ben_id, palette.to_owned()));
    assert!(!ana.send_palette(ben_id, "x".repeat(2000)));
    wait(&mut [&mut ana, &mut ben], "the palette", |p| {
        p[1].conversation(ana_id)
            .is_some_and(|c| !c.entries.is_empty())
    });
    let got = &ben.conversation(ana_id).unwrap().entries[0];
    assert_eq!(got.body, Body::Palette(palette.to_owned()));
    assert_eq!(ben.unread(ana_id), 1);
    wait(&mut [&mut ana], "the palette on the relay", |p| {
        on_relay(last_delivery(p[0], ben_id).unwrap())
    });
    let id = ana.conversation(ben_id).unwrap().entries[0].id;
    ana.delete_message(ben_id, id);
    wait(&mut [&mut ana, &mut ben], "the palette deleted", |p| {
        entry_of(p[1], ana_id, id).is_deleted()
    });
}

#[test]
fn one_tick_on_the_relay_two_when_delivered_and_read_once_seen() {
    let relay = Relay::start(1);
    let (mut ana, _, ana_id) = join(&relay, "Ana");
    let (mut ben, ben_settings, ben_id) = join(&relay, "Ben");
    befriend(&mut ana, &mut ben, ana_id, ben_id);
    ben.leave();
    wait(&mut [&mut ana, &mut ben], "Ben offline", |p| {
        !p[1].is_connected()
    });

    assert!(ana.send_text(ben_id, "are you there?"));
    wait(&mut [&mut ana], "one tick", |p| {
        last_delivery(p[0], ben_id) == Some(Delivery::Sent)
    });
    settle(&mut [&mut ana], 300);
    assert_eq!(last_delivery(&ana, ben_id), Some(Delivery::Sent));

    ben.sign_in(ben_settings.joined_relay().unwrap(), Instant::now());
    wait(&mut [&mut ana, &mut ben], "two ticks", |p| {
        matches!(last_delivery(p[0], ben_id), Some(Delivery::Delivered(_)))
    });
    assert_eq!(last_delivery(&ben, ana_id), Some(Delivery::Received));
    assert_eq!(ana.conversation(ben_id).unwrap().entries.len(), 1);
    assert_eq!(ana.unread(ben_id), 0);

    let message = ben.conversation(ana_id).unwrap().entries[0].id;
    ben.mark_seen(ana_id, &[message]);
    assert_eq!(last_delivery(&ben, ana_id), Some(Delivery::Seen));
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    wait(&mut [&mut ana, &mut ben], "read", |p| {
        matches!(last_delivery(p[0], ben_id), Some(Delivery::Read(_)))
    });
    let Some(Delivery::Read(at)) = last_delivery(&ana, ben_id) else {
        unreachable!()
    };
    assert!(at + 2 >= before, "the read time is when Ben saw it");

    assert!(ana.send_text(ben_id, "and now?"));
    wait(
        &mut [&mut ana, &mut ben],
        "the second message delivered",
        |p| matches!(last_delivery(p[0], ben_id), Some(Delivery::Delivered(_))),
    );
    let first = ana.conversation(ben_id).unwrap().entries[0].delivery;
    assert_eq!(first, Delivery::Read(at));
    assert_eq!(ana.conversation(ben_id).unwrap().entries.len(), 2);
    assert_eq!(ben.conversation(ana_id).unwrap().entries.len(), 2);
    assert_eq!(ben.unread(ana_id), 2);
}

#[test]
fn two_friends_chat_and_share_on_a_real_relay() {
    let relay = Relay::start(1);
    let (mut ana, ana_settings, ana_id) = join(&relay, "Ana");
    let (mut ben, ben_settings, ben_id) = join(&relay, "Ben");
    befriend(&mut ana, &mut ben, ana_id, ben_id);

    assert!(ana.send_text(ben_id, "gg 🍋❤️ see you at the lemon grove"));
    wait(&mut [&mut ana, &mut ben], "the first message", |p| {
        last_delivery(p[0], ben_id).is_some_and(on_relay) && !texts(p[1], ana_id).is_empty()
    });
    assert_eq!(texts(&ben, ana_id), ["gg 🍋❤️ see you at the lemon grove"]);
    assert_eq!(ben.unread(ana_id), 1);
    assert!(ben.needs_attention());
    ben.mark_read(ana_id);
    assert!(!ben.needs_attention());

    assert!(ben.send_text(ana_id, "🍋🍋"));
    wait(&mut [&mut ana, &mut ben], "the answer", |p| {
        texts(p[0], ben_id).len() == 2
    });

    let (png, rgba) = screenshot_png();
    let packed = pack_screenshot(&png).unwrap();
    assert_eq!((packed.width, packed.height), (480, 432));
    let (shot, data) = send_and_receive(
        &mut ana,
        &mut ben,
        ana_id,
        ben_id,
        share(
            ShareKind::Screenshot,
            packed.data.clone(),
            packed.width,
            packed.height,
            packed.preview.clone(),
        ),
    );
    let back = unpack_screenshot(&data, shot.width, shot.height).unwrap();
    let decoded = decode_image(&back).unwrap();
    assert_eq!((decoded.width, decoded.height), (480, 432));
    assert_eq!(decoded.rgba, rgba);
    assert!(decode_image(&shot.preview).is_some());

    let json = replay_json();
    let (replay, data) = send_and_receive(
        &mut ana,
        &mut ben,
        ana_id,
        ben_id,
        share(ShareKind::Replay, json.clone(), 160, 144, Vec::new()),
    );
    assert_eq!(data, json);
    assert_eq!(replay.rom_sha256.as_deref(), Some("ab".repeat(32).as_str()));

    let video = noise(300 << 10, 7);
    let (recording, data) = send_and_receive(
        &mut ana,
        &mut ben,
        ana_id,
        ben_id,
        share(ShareKind::Recording, video.clone(), 480, 432, Vec::new()),
    );
    assert_eq!(data, video);
    assert_eq!(recording.length_ms, Some(10_000));

    let used = ana.account().unwrap().storage_used;
    let cost = |size: u64| size.max(4096);
    assert_eq!(
        used,
        cost(shot.size) + cost(replay.size) + cost(recording.size)
    );

    assert!(ana.share(
        ben_id,
        share(
            ShareKind::Recording,
            noise(900 << 10, 9),
            480,
            432,
            Vec::new()
        )
    ));
    wait(&mut [&mut ana, &mut ben], "the full storage", |p| {
        matches!(last_delivery(p[0], ben_id), Some(Delivery::Failed(_)))
    });
    assert_eq!(
        last_delivery(&ana, ben_id),
        Some(Delivery::Failed(Failure::Refused(ErrorCode::QuotaFull)))
    );
    assert_eq!(
        Failure::Refused(ErrorCode::QuotaFull).text(),
        "Not sent, your storage is full"
    );

    ben.leave();
    wait(&mut [&mut ana, &mut ben], "Ben offline", |p| {
        !p[1].is_connected()
    });
    assert!(ana.send_text(ben_id, "while you were away"));
    wait(&mut [&mut ana], "the stored message", |p| {
        last_delivery(p[0], ben_id) == Some(Delivery::Sent)
    });
    ben.sign_in(ben_settings.joined_relay().unwrap(), Instant::now());
    wait(&mut [&mut ana, &mut ben], "the offline delivery", |p| {
        texts(p[1], ana_id)
            .iter()
            .any(|t| t == "while you were away")
    });

    ben.leave();
    wait(&mut [&mut ana, &mut ben], "Ben offline again", |p| {
        !p[1].is_connected()
    });
    let filler = "x".repeat(1_000);
    for _ in 0..200 {
        assert!(ana.send_text(ben_id, &filler));
        settle(&mut [&mut ana], 2);
    }
    assert!(ana.send_text(ben_id, "old news"));
    wait(&mut [&mut ana], "every stored message", |p| {
        p[0].conversation(ben_id)
            .unwrap()
            .entries
            .iter()
            .rev()
            .take(201)
            .all(|e| e.delivery == Delivery::Sent)
    });
    let db = relay.data().join("citrine.db");
    let full_db = std::fs::metadata(&db).unwrap().len() + wal(&db);

    let all = files_in(&relay.data());
    let markers: [&[u8]; 6] = [
        b"lemon grove",
        b"while you were away",
        b"old news",
        b"xxxxxxxxxxxxxxxx",
        b"rom_sha256",
        b"\x89PNG",
    ];
    for path in &all {
        let bytes = std::fs::read(path).unwrap();
        for marker in markers {
            assert!(
                !contains(&bytes, marker),
                "{} holds {:?} in the clear",
                path.display(),
                String::from_utf8_lossy(marker)
            );
        }
        assert!(
            !contains(&bytes, &video[..64]),
            "{} holds the video",
            path.display()
        );
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    relay.tidy(now + 31 * DAY);
    let small_db = std::fs::metadata(&db).unwrap().len() + wal(&db);
    ben.sign_in(ben_settings.joined_relay().unwrap(), Instant::now());
    wait(&mut [&mut ana, &mut ben], "the month old message", |p| {
        texts(p[1], ana_id).iter().any(|t| t == "old news")
    });

    let files_dir = relay.data().join("files");
    let stored = bytes_in(&files_dir);
    assert_eq!(stored, shot.size + replay.size + recording.size);
    assert!(ana.list_files());
    wait(&mut [&mut ana], "the file list", |p| {
        p[0].shared_files().is_some_and(|f| f.len() == 3)
    });
    let ids: Vec<FileId> = ana.shared_files().unwrap().iter().map(|f| f.id).collect();
    for id in &ids {
        assert!(ana.share_of(*id).is_some_and(|(to, _)| to == ben_id));
        assert!(ana.delete_file(*id));
    }
    wait(&mut [&mut ana], "the deletes", |p| {
        p[0].shared_files().is_some_and(|f| f.is_empty())
            && p[0].account().unwrap().storage_used == 0
    });
    assert_eq!(bytes_in(&files_dir), 0);
    ben.fetch(ana_id, shot.file);
    wait(&mut [&mut ana, &mut ben], "the gone file", |p| {
        matches!(p[1].fetched(shot.file), Some(Fetch::Failed(_)))
    });

    let keys = ana_settings.joined_relay().unwrap().relay_id.keys();
    let ben_key = ana.friend(ben_id).unwrap().public_key;
    let message = LETTER.to_owned();
    assert!((950..=1050).contains(&message.len()));
    let sealed_message = keys
        .seal_envelope(&ben_key, &Envelope::new(Body::Text(message.clone())))
        .to_bytes()
        .len();
    println!(
        "sizes: screenshot png {} bytes, packed {} bytes, sealed {} bytes",
        png.len(),
        packed.data.len(),
        shot.size
    );
    println!(
        "sizes: message {} bytes, sealed {} bytes",
        message.len(),
        sealed_message
    );
    println!(
        "sizes: replay json {} bytes, sealed {} bytes",
        json.len(),
        replay.size
    );
    println!(
        "sizes: recording {} bytes, sealed {} bytes",
        video.len(),
        recording.size
    );
    println!("sizes: database {full_db} bytes before the tidy, {small_db} after");
}

fn wal(db: &Path) -> u64 {
    let mut name = db.as_os_str().to_owned();
    name.push("-wal");
    std::fs::metadata(name).map_or(0, |m| m.len())
}
