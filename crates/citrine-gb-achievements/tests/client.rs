#![cfg(not(target_arch = "wasm32"))]

use citrine_gb_achievements::{
    Client, Console, Event, GroupKind, Memory, NoMemory, Request, Response, SignInError, hash_rom,
};

const LOGIN: &str = r#"{"Success":true,"User":"User","Token":"ApiToken","Score":12345,"SoftcoreScore":123,"Messages":2,"Permissions":1,"AccountType":"Registered"}"#;
const WRONG_PASSWORD: &str = r#"{"Success":false,"Error":"Invalid User/Password combination. Please try again","Status":401,"Code":"invalid_credentials"}"#;
const EXPIRED: &str = r#"{"Success":false,"Error":"The access token has expired. Please log in again.","Status":401,"Code":"expired_token"}"#;
const NOT_FOUND: &str =
    r#"{"Success":false,"Error":"Unknown game","Code":"not_found","Status":404}"#;
const NO_UNLOCKS: &str = r#"{"Success":true,"Unlocks":[],"HardcoreUnlocks":[]}"#;
const ONE_UNLOCK: &str =
    r#"{"Success":true,"Unlocks":[{"ID":5502,"When":1234567890}],"HardcoreUnlocks":[]}"#;
const AWARD: &str = r#"{"Success":true,"Score":12345,"SoftcoreScore":128,"AchievementID":5501,"AchievementsRemaining":1}"#;

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

fn answer(client: &mut Client, request: Request, status: i32, body: &str, memory: &mut dyn Memory) {
    client.complete(
        request.id,
        Response {
            status,
            body: body.as_bytes().to_vec(),
        },
        memory,
    );
}

fn signed_in() -> Client {
    let mut client = Client::new();
    client.sign_in_with_password("User", "Pa$$word");
    let login = one_request(&mut client, "r=login2&u=User&p=Pa%24%24word");
    answer(&mut client, login, 200, LOGIN, &mut NoMemory);
    client.take_events();
    client
}

fn loaded(unlocks: &str, ram: &mut Ram) -> Client {
    let mut client = signed_in();
    client.load_game(Console::GameBoy, b"citrine test rom", ram);
    let set = one_request(&mut client, "r=achievementsets&u=User&t=ApiToken&m=");
    answer(&mut client, set, 200, &game_set(), ram);
    let session = one_request(&mut client, "r=startsession&u=User&t=ApiToken&g=1234&h=0");
    answer(&mut client, session, 200, unlocks, ram);
    client
}

#[test]
fn password_sign_in_gives_the_token_and_never_keeps_the_password() {
    let mut client = Client::new();
    client.sign_in_with_password("User", "Pa$$word");
    let login = one_request(&mut client, "r=login2&u=User&p=Pa%24%24word");
    assert!(
        login
            .url
            .starts_with("https://retroachievements.org/dorequest.php")
    );
    answer(&mut client, login, 200, LOGIN, &mut NoMemory);

    let events = client.take_events();
    let [Event::SignedIn(user)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(user.username, "User");
    assert_eq!(user.token, "ApiToken");
    assert_eq!(user.points, 123);
    assert_eq!(client.user().as_ref(), Some(user));
}

#[test]
fn a_wrong_password_says_so() {
    let mut client = Client::new();
    client.sign_in_with_password("User", "Pa$$word");
    let login = one_request(&mut client, "r=login2");
    answer(&mut client, login, 401, WRONG_PASSWORD, &mut NoMemory);
    let events = client.take_events();
    assert!(
        matches!(
            events.as_slice(),
            [Event::SignInFailed {
                error: SignInError::WrongPassword,
                ..
            }]
        ),
        "{events:?}"
    );
    assert_eq!(client.user(), None);
}

#[test]
fn an_expired_token_asks_for_the_password_again() {
    let mut client = Client::new();
    client.sign_in_with_token("User", "EXPIRED");
    let login = one_request(&mut client, "r=login2&u=User&t=EXPIRED");
    answer(&mut client, login, 403, EXPIRED, &mut NoMemory);
    let events = client.take_events();
    assert!(
        matches!(
            events.as_slice(),
            [Event::SignInFailed {
                error: SignInError::TokenExpired,
                ..
            }]
        ),
        "{events:?}"
    );
}

#[test]
fn a_network_failure_is_not_a_wrong_password() {
    let mut client = Client::new();
    client.sign_in_with_token("User", "ApiToken");
    let login = one_request(&mut client, "r=login2");
    client.complete(login.id, Response::network_error(), &mut NoMemory);
    let events = client.take_events();
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::SignInFailed {
                error: SignInError::WrongPassword | SignInError::TokenExpired,
                ..
            }
        )),
        "{events:?}"
    );
}

#[test]
fn a_loaded_set_lists_locked_and_unlocked_achievements() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(ONE_UNLOCK, &mut ram);
    let events = client.take_events();
    assert_eq!(events, vec![Event::GameLoaded]);

    let game = client.game().expect("a game");
    assert_eq!(game.id, 1234);
    assert_eq!(game.title, "Sample Game");
    assert_eq!(game.summary.total, 2);
    assert_eq!(game.summary.unlocked, 1);
    assert_eq!(game.summary.points_total, 7);
    assert_eq!(game.summary.points_unlocked, 2);

    let locked = game
        .groups
        .iter()
        .find(|g| g.kind == GroupKind::Locked)
        .unwrap();
    assert_eq!(locked.achievements.len(), 1);
    let ach1 = &locked.achievements[0];
    assert_eq!(ach1.title, "Ach1");
    assert!(!ach1.unlocked);
    assert_eq!(
        ach1.shown_badge_url(),
        Some("https://media.retroachievements.org/Badge/00234_lock.png")
    );

    let unlocked: Vec<_> = game
        .groups
        .iter()
        .filter(|g| matches!(g.kind, GroupKind::Unlocked | GroupKind::RecentlyUnlocked))
        .flat_map(|g| &g.achievements)
        .collect();
    assert_eq!(unlocked.len(), 1);
    assert_eq!(
        unlocked[0].shown_badge_url(),
        Some("https://media.retroachievements.org/Badge/00235.png")
    );
}

#[test]
fn a_trigger_unlocks_and_sends_the_award() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = loaded(NO_UNLOCKS, &mut ram);
    client.take_events();
    client.take_requests();

    client.do_frame(&mut ram);
    assert!(client.take_events().is_empty());
    ram.0[1] = 3;
    ram.0[2] = 7;
    client.do_frame(&mut ram);

    let events = client.take_events();
    let [Event::Unlocked(achievement)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(achievement.id, 5501);
    assert_eq!(achievement.points, 5);
    assert!(achievement.unlocked);

    let award = one_request(
        &mut client,
        "r=awardachievement&u=User&t=ApiToken&a=5501&h=0",
    );
    answer(&mut client, award, 200, AWARD, &mut ram);
    assert_eq!(client.waiting_on(), 0);
    assert_eq!(client.game().unwrap().summary.unlocked, 1);
}

fn triggered(ram: &mut Ram) -> Client {
    let mut client = loaded(NO_UNLOCKS, ram);
    client.take_events();
    client.take_requests();
    client.do_frame(ram);
    ram.0[1] = 3;
    ram.0[2] = 7;
    client.do_frame(ram);
    client.take_events();
    client
}

#[test]
fn an_unlock_counts_as_unsent_until_the_server_takes_it() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = triggered(&mut ram);
    assert_eq!(client.unsent_unlocks(), 1);

    let award = one_request(&mut client, "r=awardachievement&u=User&t=ApiToken&a=5501");
    client.complete(award.id, Response::network_error(), &mut ram);
    assert_eq!(client.unsent_unlocks(), 1);
    let retry = one_request(&mut client, "r=awardachievement&u=User&t=ApiToken&a=5501");
    answer(&mut client, retry, 503, "", &mut ram);
    assert_eq!(client.unsent_unlocks(), 1);
    assert!(client.take_requests().is_empty());

    std::thread::sleep(std::time::Duration::from_millis(1100));
    client.idle(&mut ram);
    let later = one_request(&mut client, "r=awardachievement&u=User&t=ApiToken&a=5501");
    answer(&mut client, later, 200, AWARD, &mut ram);
    assert_eq!(client.unsent_unlocks(), 0);
}

#[test]
fn a_refused_unlock_is_not_unsent() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = triggered(&mut ram);
    let award = one_request(&mut client, "r=awardachievement&u=User&t=ApiToken&a=5501");
    answer(&mut client, award, 404, NOT_FOUND, &mut ram);
    assert_eq!(client.unsent_unlocks(), 0);
    assert!(client.take_requests().is_empty());
}

#[test]
fn the_last_unlock_sent_after_the_game_closed_is_safe() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = triggered(&mut ram);
    let award = one_request(&mut client, "r=awardachievement&u=User&t=ApiToken&a=5501");
    client.unload_game();
    let last = AWARD.replace(
        r#""AchievementsRemaining":1"#,
        r#""AchievementsRemaining":0"#,
    );
    answer(&mut client, award, 200, &last, &mut ram);
    assert_eq!(client.unsent_unlocks(), 0);
    assert_eq!(client.waiting_on(), 0);
}

#[test]
fn signing_out_drops_unsent_unlocks() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = triggered(&mut ram);
    assert_eq!(client.unsent_unlocks(), 1);
    client.sign_out();
    assert_eq!(client.unsent_unlocks(), 0);
}

#[test]
fn frames_run_only_while_a_set_is_loaded() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = signed_in();
    assert!(!client.runs_frames());
    client.load_game(Console::GameBoy, b"citrine test rom", &mut ram);
    assert!(!client.runs_frames());
    let set = one_request(&mut client, "r=achievementsets");
    answer(&mut client, set, 200, &game_set(), &mut ram);
    let session = one_request(&mut client, "r=startsession");
    answer(&mut client, session, 200, NO_UNLOCKS, &mut ram);
    assert!(client.runs_frames());
    client.unload_game();
    assert!(!client.runs_frames());
}

#[test]
fn loading_a_game_while_signed_out_asks_nothing() {
    let mut client = Client::new();
    client.load_game(Console::GameBoy, b"citrine test rom", &mut NoMemory);
    assert!(client.take_requests().is_empty());
    assert!(client.take_events().is_empty());
}

#[test]
fn a_game_without_a_set_says_so() {
    let mut ram = Ram(vec![0; 0x10000]);
    let mut client = signed_in();
    client.load_game(Console::GameBoyColor, b"homebrew", &mut ram);
    let set = one_request(&mut client, "r=achievementsets");
    answer(&mut client, set, 404, NOT_FOUND, &mut ram);
    assert_eq!(client.take_events(), vec![Event::NoSet]);
}

#[test]
fn the_user_agent_names_citrine_and_rcheevos() {
    let client = Client::new();
    let agent = client.user_agent("Citrine/0.12.0 (Linux)");
    assert!(
        agent.starts_with("Citrine/0.12.0 (Linux) rcheevos/12.5"),
        "{agent}"
    );
}

#[test]
fn a_game_boy_hash_is_the_md5_of_the_rom() {
    assert_eq!(
        hash_rom(Console::GameBoy, b"abc").as_deref(),
        Some("900150983cd24fb0d6963f7d28e17f72")
    );
}

#[test]
fn library_progress_comes_per_console() {
    let mut client = signed_in();
    client.fetch_progress(Console::GameBoy);
    let request = one_request(&mut client, "r=allprogress&u=User&t=ApiToken&c=4");
    answer(
        &mut client,
        request,
        200,
        r#"{"Success":true,"Response":{"724":{"Achievements":93,"Unlocked":4},"10":{"Achievements":11}}}"#,
        &mut NoMemory,
    );
    let events = client.take_events();
    let [Event::AllProgress { console, games }] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(*console, Console::GameBoy);
    let red = games.iter().find(|g| g.game_id == 724).unwrap();
    assert_eq!((red.unlocked, red.total), (4, 93));
}
