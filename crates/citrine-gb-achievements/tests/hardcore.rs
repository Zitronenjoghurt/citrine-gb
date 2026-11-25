#![cfg(not(target_arch = "wasm32"))]

use citrine_gb_achievements::{
    Client, Console, Event, GroupKind, Memory, NoMemory, Request, Response,
};

const LOGIN: &str = r#"{"Success":true,"User":"User","Token":"ApiToken","Score":12345,"SoftcoreScore":123,"Messages":2,"Permissions":1,"AccountType":"Registered"}"#;
const NO_UNLOCKS: &str = r#"{"Success":true,"Unlocks":[],"HardcoreUnlocks":[]}"#;
const SOFTCORE_UNLOCK: &str =
    r#"{"Success":true,"Unlocks":[{"ID":5502,"When":1234567890}],"HardcoreUnlocks":[]}"#;

fn game_set() -> String {
    String::from(concat!(
        r#"{"Success":true,"GameId":1234,"Title":"Sample Game","ConsoleId":4,"#,
        r#""ImageIconUrl":"http://server/Images/112233.png","#,
        r#""RichPresenceGameId":1234,"RichPresencePatch":"","Sets":[{"#,
        r#""AchievementSetId":1111,"GameId":1234,"Title":null,"Type":"core","#,
        r#""ImageIconUrl":"http://server/Images/112233.png","Achievements":["#,
        r#"{"ID":5501,"Title":"Ach1","Description":"Desc1","Flags":3,"Points":5,"#,
        r#""MemAddr":"0xH0001=3_0xH0002=7","Author":"User1","BadgeName":"00234","#,
        r#""Created":1367266583,"Modified":1376929305},"#,
        r#"{"ID":5502,"Title":"Ach2","Description":"Desc2","Flags":3,"Points":2,"#,
        r#""MemAddr":"0xH0001=2_0x0002=9","Author":"User1","BadgeName":"00235","#,
        r#""Created":1376970283,"Modified":1376970283},"#,
        r#"{"ID":5503,"Title":"Ach3","Description":"Stand on the switch for three frames","Flags":3,"Points":10,"#,
        r#""MemAddr":"0xH0003=1.3.","Author":"User1","BadgeName":"00236","#,
        r#""Created":1376970283,"Modified":1376970283}"#,
        r#"],"Leaderboards":[]}]}"#
    ))
}

struct Ram(Vec<u8>);

impl Memory for Ram {
    fn read(&mut self, address: u32, buffer: &mut [u8]) -> usize {
        let start = address as usize;
        let Some(bytes) = self.0.get(start..start + buffer.len()) else {
            return 0;
        };
        buffer.copy_from_slice(bytes);
        buffer.len()
    }
}

fn post(request: &Request) -> &str {
    request.post_data.as_deref().unwrap_or("")
}

fn one_request(client: &mut Client, starts: &str) -> Request {
    let requests = client.take_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = requests.into_iter().next().unwrap();
    assert!(post(&request).starts_with(starts), "{}", post(&request));
    request
}

fn answer(client: &mut Client, request: Request, body: &str, memory: &mut dyn Memory) {
    client.complete(request.id, Response::ok(body), memory);
}

fn loaded(hardcore: bool, unlocks: &str, ram: &mut Ram) -> Client {
    let mut client = Client::new();
    client.set_hardcore(hardcore);
    client.sign_in_with_password("User", "Pa$$word");
    let login = one_request(&mut client, "r=login2");
    answer(&mut client, login, LOGIN, &mut NoMemory);
    client.load_game(Console::GameBoy, b"citrine test rom", ram);
    let set = one_request(&mut client, "r=achievementsets");
    answer(&mut client, set, &game_set(), ram);
    let session = one_request(&mut client, "r=startsession");
    answer(&mut client, session, unlocks, ram);
    client.take_events();
    client.take_requests();
    client.do_frame(ram);
    client
}

fn unlocked_ids(events: &[Event]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Unlocked(achievement) => Some(achievement.id),
            _ => None,
        })
        .collect()
}

fn awards(client: &mut Client) -> Vec<String> {
    client
        .take_requests()
        .iter()
        .map(|request| post(request).to_owned())
        .filter(|post| post.starts_with("r=awardachievement"))
        .collect()
}

#[test]
fn hardcore_is_off_until_asked_for() {
    let client = Client::new();
    assert!(!client.hardcore());
}

#[test]
fn a_hardcore_session_and_its_unlocks_say_hardcore() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = Client::new();
    client.set_hardcore(true);
    assert!(client.hardcore());
    client.sign_in_with_password("User", "Pa$$word");
    let login = one_request(&mut client, "r=login2");
    answer(&mut client, login, LOGIN, &mut NoMemory);
    client.load_game(Console::GameBoy, b"citrine test rom", &mut ram);
    let set = one_request(&mut client, "r=achievementsets");
    answer(&mut client, set, &game_set(), &mut ram);
    let session = one_request(&mut client, "r=startsession");
    assert!(post(&session).contains("&h=1"), "{}", post(&session));
    answer(&mut client, session, NO_UNLOCKS, &mut ram);

    client.do_frame(&mut ram);
    ram.0[1] = 3;
    ram.0[2] = 7;
    client.do_frame(&mut ram);
    assert_eq!(unlocked_ids(&client.take_events()), vec![5501]);
    let sent = awards(&mut client);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("a=5501&h=1"), "{sent:?}");
}

#[test]
fn turning_hardcore_on_mid_game_waits_for_a_reset() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(false, NO_UNLOCKS, &mut ram);
    client.set_hardcore(true);
    assert!(client.take_events().contains(&Event::ResetNeeded));

    ram.0[1] = 3;
    ram.0[2] = 7;
    client.do_frame(&mut ram);
    client.do_frame(&mut ram);
    assert!(unlocked_ids(&client.take_events()).is_empty());

    ram.0[1] = 0;
    ram.0[2] = 0;
    client.reset();
    client.do_frame(&mut ram);
    ram.0[1] = 3;
    ram.0[2] = 7;
    client.do_frame(&mut ram);
    assert_eq!(unlocked_ids(&client.take_events()), vec![5501]);
    let sent = awards(&mut client);
    assert!(sent.iter().any(|post| post.contains("&h=1")), "{sent:?}");
}

#[test]
fn turning_hardcore_off_needs_no_reset() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(true, NO_UNLOCKS, &mut ram);
    client.set_hardcore(false);
    assert!(!client.take_events().contains(&Event::ResetNeeded));
    ram.0[1] = 3;
    ram.0[2] = 7;
    client.do_frame(&mut ram);
    assert_eq!(unlocked_ids(&client.take_events()), vec![5501]);
    assert!(awards(&mut client).iter().all(|post| post.contains("&h=0")));
}

#[test]
fn a_softcore_unlock_shows_locked_in_hardcore() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(false, SOFTCORE_UNLOCK, &mut ram);
    let unlocked = |client: &Client| {
        client
            .game()
            .unwrap()
            .groups
            .iter()
            .filter(|group| {
                matches!(
                    group.kind,
                    GroupKind::Unlocked | GroupKind::RecentlyUnlocked
                )
            })
            .flat_map(|group| &group.achievements)
            .map(|achievement| (achievement.id, achievement.unlocked))
            .collect::<Vec<_>>()
    };
    assert_eq!(unlocked(&client), vec![(5502, true)]);
    assert_eq!(client.game().unwrap().summary.unlocked, 1);

    client.set_hardcore(true);
    assert!(unlocked(&client).is_empty());
    let game = client.game().unwrap();
    assert_eq!(game.summary.unlocked, 0);
    let ach2 = game
        .groups
        .iter()
        .flat_map(|group| &group.achievements)
        .find(|achievement| achievement.id == 5502)
        .unwrap();
    assert!(!ach2.unlocked);
}

#[test]
fn softcore_can_always_pause() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(false, NO_UNLOCKS, &mut ram);
    for _ in 0..5 {
        assert_eq!(client.can_pause(), Ok(()));
        client.do_frame(&mut ram);
    }
}

#[test]
fn hardcore_refuses_a_pause_right_after_the_last_one() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(true, NO_UNLOCKS, &mut ram);
    assert_eq!(client.can_pause(), Ok(()));
    for _ in 0..5 {
        client.do_frame(&mut ram);
    }
    let Err(wait) = client.can_pause() else {
        panic!("a pause five frames after the last one went through");
    };
    assert!(wait > 0);
    for _ in 0..wait {
        client.do_frame(&mut ram);
    }
    assert_eq!(client.can_pause(), Ok(()));
}

fn stand_on_switch(client: &mut Client, ram: &mut Ram, frames: usize) -> Vec<u32> {
    ram.0[3] = 1;
    let mut unlocked = Vec::new();
    for _ in 0..frames {
        client.do_frame(ram);
        unlocked.extend(unlocked_ids(&client.take_events()));
    }
    ram.0[3] = 0;
    unlocked
}

#[test]
fn restored_progress_keeps_the_hits_so_far() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(false, NO_UNLOCKS, &mut ram);
    assert!(stand_on_switch(&mut client, &mut ram, 2).is_empty());
    let saved = client.progress().expect("progress");

    client.restore_progress(None);
    assert!(stand_on_switch(&mut client, &mut ram, 1).is_empty());

    client.restore_progress(Some(&saved));
    assert_eq!(stand_on_switch(&mut client, &mut ram, 1), vec![5503]);
}

#[test]
fn going_back_before_an_unlock_never_unlocks_it_twice() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(false, NO_UNLOCKS, &mut ram);
    let before = client.progress().expect("progress");
    assert_eq!(stand_on_switch(&mut client, &mut ram, 3), vec![5503]);
    assert_eq!(awards(&mut client).len(), 1);

    client.restore_progress(Some(&before));
    assert!(stand_on_switch(&mut client, &mut ram, 3).is_empty());
    assert!(awards(&mut client).is_empty());
}

#[test]
fn progress_needs_a_loaded_set() {
    let mut client = Client::new();
    assert_eq!(client.progress(), None);
    client.restore_progress(Some(&[1, 2, 3]));
}
