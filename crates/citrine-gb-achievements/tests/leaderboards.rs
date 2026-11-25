#![cfg(not(target_arch = "wasm32"))]

use citrine_gb_achievements::{
    Attempt, Client, Console, Entry, Event, Fetch, Memory, NoMemory, Request, Response, ScoreFormat,
};

const LOGIN: &str = r#"{"Success":true,"User":"User","Token":"ApiToken","Score":12345,"SoftcoreScore":123,"Messages":2,"Permissions":1,"AccountType":"Registered"}"#;
const NO_UNLOCKS: &str = r#"{"Success":true,"Unlocks":[],"HardcoreUnlocks":[]}"#;
const SUBMITTED: &str = concat!(
    r#"{"Success":true,"Response":{"Score":42,"BestScore":42,"#,
    r#""RankInfo":{"Rank":2,"NumEntries":"3"},"#,
    r#""TopEntries":[{"User":"Top","Rank":1,"Score":99},{"User":"User","Rank":2,"Score":42}]}}"#
);
const ENTRIES: &str = concat!(
    r#"{"Success":true,"LeaderboardData":{"LBID":7001,"LBFormat":"VALUE","LowerIsBetter":0,"#,
    r#""LBTitle":"Floors","LBDesc":"Climb as high as you can","LBMem":"","GameID":1234,"#,
    r#""LBAuthor":"User1","LBCreated":"2013-10-20 22:12:21","LBUpdated":"2021-06-14 08:18:19","#,
    r#""Entries":[{"User":"Top","Rank":1,"Index":1,"Score":99,"DateSubmitted":1615654895},"#,
    r#"{"User":"User","Rank":2,"Index":2,"Score":42,"DateSubmitted":1615654896}],"#,
    r#""TotalEntries":3}}"#
);

fn game_set(presence: &str) -> String {
    format!(
        concat!(
            r#"{{"Success":true,"GameId":1234,"Title":"Sample Game","ConsoleId":4,"#,
            r#""ImageIconUrl":"http://server/Images/112233.png","#,
            r#""RichPresenceGameId":1234,"RichPresencePatch":"{}","Sets":[{{"#,
            r#""AchievementSetId":1111,"GameId":1234,"Title":null,"Type":"core","#,
            r#""ImageIconUrl":"http://server/Images/112233.png","Achievements":["#,
            r#"{{"ID":5501,"Title":"Ach1","Description":"Desc1","Flags":3,"Points":5,"#,
            r#""MemAddr":"0xH0001=3_0xH0002=7","Author":"User1","BadgeName":"00234","#,
            r#""Created":1367266583,"Modified":1376929305}}"#,
            r#"],"Leaderboards":["#,
            r#"{{"ID":7001,"Title":"Floors","Description":"Climb as high as you can","#,
            r#""Mem":"STA:0xH0003=1::CAN:0xH0003=2::SUB:0xH0003=3::VAL:0xH0004","#,
            r#""Format":"VALUE","LowerIsBetter":false,"Hidden":false}},"#,
            r#"{{"ID":7002,"Title":"Speedrun","Description":"Reach the end fast","#,
            r#""Mem":"STA:0xH0006=1::CAN:0xH0006=2::SUB:0xH0006=3::VAL:0xH0007","#,
            r#""Format":"FRAMES","LowerIsBetter":true,"Hidden":false}}"#,
            r#"]}}]}}"#
        ),
        presence
    )
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

fn loaded_with(hardcore: bool, presence: &str, ram: &mut Ram) -> Client {
    let mut client = Client::new();
    client.set_hardcore(hardcore);
    client.sign_in_with_password("User", "Pa$$word");
    let login = one_request(&mut client, "r=login2");
    answer(&mut client, login, LOGIN, &mut NoMemory);
    client.load_game(Console::GameBoy, b"citrine test rom", ram);
    let set = one_request(&mut client, "r=achievementsets");
    answer(&mut client, set, &game_set(presence), ram);
    let session = one_request(&mut client, "r=startsession");
    answer(&mut client, session, NO_UNLOCKS, ram);
    client.take_events();
    client.take_requests();
    client.do_frame(ram);
    client
}

fn loaded(hardcore: bool, ram: &mut Ram) -> Client {
    loaded_with(hardcore, "", ram)
}

fn frame(client: &mut Client, ram: &mut Ram) -> Vec<Event> {
    client.do_frame(ram);
    client.take_events()
}

#[test]
fn a_loaded_set_lists_its_leaderboards() {
    let mut ram = Ram(vec![0; 0x100]);
    let client = loaded(false, &mut ram);
    let boards = client.game().unwrap().leaderboards;
    let titles: Vec<_> = boards.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(titles, ["Floors", "Speedrun"]);
    assert_eq!(boards[0].id, 7001);
    assert_eq!(boards[0].description, "Climb as high as you can");
    assert_eq!(boards[0].format, ScoreFormat::Value);
    assert!(!boards[0].lower_is_better);
    assert_eq!(boards[1].format, ScoreFormat::Time);
    assert!(boards[1].lower_is_better);
}

#[test]
fn a_hardcore_attempt_starts_tracks_and_submits_with_a_rank() {
    let mut ram = Ram(vec![0; 0x100]);
    let mut client = loaded(true, &mut ram);

    ram.0[3] = 1;
    ram.0[4] = 5;
    let events = frame(&mut client, &mut ram);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::BoardStarted(b) if b.id == 7001)),
        "{events:?}"
    );
    assert!(events.contains(&Event::TrackersChanged));
    assert!(client.game().unwrap().leaderboards[0].tracking);
    assert_eq!(
        client.attempts(),
        [Attempt {
            board: 7001,
            title: "Floors".into(),
            score: "5".into()
        }]
    );

    ram.0[4] = 42;
    let events = frame(&mut client, &mut ram);
    assert_eq!(events, [Event::TrackersChanged]);
    assert_eq!(client.attempts()[0].score, "42");

    ram.0[3] = 3;
    let events = frame(&mut client, &mut ram);
    assert!(
        events.iter().any(
            |e| matches!(e, Event::BoardSubmitted { board, score } if board.id == 7001 && score == "42")
        ),
        "{events:?}"
    );
    assert!(events.contains(&Event::TrackersChanged), "{events:?}");
    assert!(client.attempts().is_empty());

    let submit = one_request(&mut client, "r=submitlbentry&u=User&t=ApiToken&i=7001&s=42");
    answer(&mut client, submit, SUBMITTED, &mut ram);
    let events = client.take_events();
    let [Event::Scoreboard(board)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(board.board, 7001);
    assert_eq!(board.submitted, "42");
    assert_eq!(board.best, "42");
    assert_eq!(board.rank, 2);
    assert_eq!(board.entries, 3);
    assert_eq!(
        board.top,
        [
            Entry {
                rank: 1,
                user: "Top".into(),
                score: "99".into()
            },
            Entry {
                rank: 2,
                user: "User".into(),
                score: "42".into()
            },
        ]
    );
}

#[test]
fn two_running_attempts_stack() {
    let mut ram = Ram(vec![0; 0x100]);
    let mut client = loaded(true, &mut ram);
    ram.0[3] = 1;
    ram.0[4] = 7;
    ram.0[6] = 1;
    ram.0[7] = 120;
    frame(&mut client, &mut ram);
    let attempts = client.attempts();
    let boards: Vec<_> = attempts.iter().map(|a| a.board).collect();
    assert_eq!(boards, [7001, 7002]);
    assert_eq!(attempts[0].score, "7");
    assert_eq!(attempts[1].score, "0:02.00");
}

#[test]
fn a_cancelled_attempt_fails_and_sends_nothing() {
    let mut ram = Ram(vec![0; 0x100]);
    let mut client = loaded(true, &mut ram);
    ram.0[3] = 1;
    frame(&mut client, &mut ram);
    ram.0[3] = 2;
    let events = frame(&mut client, &mut ram);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::BoardFailed(b) if b.id == 7001)),
        "{events:?}"
    );
    assert!(events.contains(&Event::TrackersChanged), "{events:?}");
    assert!(client.attempts().is_empty());
    assert!(client.take_requests().is_empty());
}

#[test]
fn softcore_starts_no_attempt() {
    let mut ram = Ram(vec![0; 0x100]);
    let mut client = loaded(false, &mut ram);
    ram.0[3] = 1;
    let events = frame(&mut client, &mut ram);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::BoardStarted(_) | Event::TrackersChanged)),
        "{events:?}"
    );
    ram.0[3] = 3;
    frame(&mut client, &mut ram);
    assert!(client.take_requests().is_empty());
    assert!(client.attempts().is_empty());
}

#[test]
fn the_top_entries_mark_the_signed_in_player() {
    let mut ram = Ram(vec![0; 0x100]);
    let mut client = loaded(false, &mut ram);
    client.fetch_entries(7001, Fetch::Top, 10);
    let request = one_request(&mut client, "r=lbinfo&i=7001&c=10");
    answer(&mut client, request, ENTRIES, &mut ram);
    let events = client.take_events();
    let [Event::Entries(entries)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(entries.board, 7001);
    assert_eq!(entries.fetch, Fetch::Top);
    assert_eq!(entries.total, 3);
    assert_eq!(entries.mine, Some(1));
    assert_eq!(
        entries.entries[0],
        Entry {
            rank: 1,
            user: "Top".into(),
            score: "99".into()
        }
    );
}

#[test]
fn entries_around_the_player_ask_by_username() {
    let mut ram = Ram(vec![0; 0x100]);
    let mut client = loaded(false, &mut ram);
    client.fetch_entries(7001, Fetch::AroundMe, 5);
    let request = one_request(&mut client, "r=lbinfo&i=7001&u=User&c=5");
    answer(&mut client, request, ENTRIES, &mut ram);
    let events = client.take_events();
    let [Event::Entries(entries)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!(entries.fetch, Fetch::AroundMe);
    assert_eq!(entries.entries[entries.mine.unwrap()].score, "42");
}

#[test]
fn a_failed_fetch_names_its_board() {
    let mut ram = Ram(vec![0; 0x100]);
    let mut client = loaded(false, &mut ram);
    client.fetch_entries(7002, Fetch::Top, 10);
    let request = client.take_requests().pop().unwrap();
    client.complete(request.id, Response::network_error(), &mut ram);
    let events = client.take_events();
    assert!(
        matches!(
            events.as_slice(),
            [Event::EntriesFailed {
                board: 7002,
                fetch: Fetch::Top,
                ..
            }]
        ),
        "{events:?}"
    );
}

#[test]
fn rich_presence_follows_the_game() {
    let mut ram = Ram(vec![0; 0x100]);
    let script = r"Format:Num\nFormatType=VALUE\n\nDisplay:\nOn floor @Num(0xH0005)";
    let mut client = loaded_with(false, script, &mut ram);
    ram.0[5] = 3;
    frame(&mut client, &mut ram);
    assert_eq!(client.rich_presence().as_deref(), Some("On floor 3"));
}

#[test]
fn a_game_without_a_presence_script_has_none() {
    let mut ram = Ram(vec![0; 0x100]);
    let mut client = loaded(false, &mut ram);
    frame(&mut client, &mut ram);
    assert_eq!(client.rich_presence(), None);
}
