use citrine_gb_achievements::{Attempt, Entries, Fetch, GameState, ScoreFormat, Scoreboard, User};
use std::collections::BTreeSet;

pub use citrine_gb_achievements::Entry;

pub const TOP: u32 = 10;
pub const AROUND: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub format: ScoreFormat,
    pub best: Best,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Best {
    #[default]
    Unknown,
    NoScore,
    Score(Entry),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rankings {
    pub board: u32,
    pub entries: u32,
    pub top: Vec<Entry>,
    pub around: Vec<Entry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Achievements,
    Leaderboards,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardNotice {
    Started,
    Failed,
    Submitted {
        score: String,
        rank: u32,
        entries: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankingsState {
    Loading,
    Failed,
    Shown,
}

#[derive(Debug, Default)]
pub struct Boards {
    pub list: Vec<Board>,
    pub rankings: Option<Rankings>,
    pub open: Option<u32>,
    pub view: View,
    pub presence: Option<String>,
    pub attempts: Vec<Attempt>,
    pub scores_sent: bool,
    pub hardcore: bool,
    game: Option<u32>,
    asked_best: BTreeSet<u32>,
    asked_rankings: Option<u32>,
    rankings_failed: bool,
    wants: Vec<(u32, Fetch, u32)>,
}

pub fn me(user: &User) -> &str {
    if user.display_name.is_empty() {
        &user.username
    } else {
        &user.display_name
    }
}

pub const NOT_SENT: &str = "Scores aren't sent until RetroAchievements approves Citrine";
pub const NEEDS_HARDCORE: &str = "Leaderboards need hardcore mode";
pub const PRESENCE_HINT: &str = "Your RetroAchievements profile shows this while you play";

impl Boards {
    pub fn board(&self, id: u32) -> Option<&Board> {
        self.list.iter().find(|board| board.id == id)
    }

    fn board_mut(&mut self, id: u32) -> Option<&mut Board> {
        self.list.iter_mut().find(|board| board.id == id)
    }

    pub fn open_title(&self) -> Option<&str> {
        self.open
            .and_then(|id| self.board(id))
            .map(|board| board.title.as_str())
    }

    pub fn tabs(&self) -> bool {
        !self.list.is_empty()
    }

    pub fn note(&self) -> Option<&'static str> {
        if !self.hardcore {
            Some(NEEDS_HARDCORE)
        } else if !self.scores_sent {
            Some(NOT_SENT)
        } else {
            None
        }
    }

    pub fn tracker_lines(&self) -> Vec<(String, String)> {
        self.attempts
            .iter()
            .map(|attempt| (attempt.title.clone(), attempt.score.clone()))
            .collect()
    }

    pub fn layout_lines(&self) -> Vec<(String, String)> {
        if !self.attempts.is_empty() || !self.hardcore {
            return self.tracker_lines();
        }
        let scored = self.list.iter().find_map(|board| match &board.best {
            Best::Score(entry) => Some((board.title.clone(), entry.score.clone())),
            _ => None,
        });
        let first = || {
            self.list.first().map(|board| {
                let zero = match board.format {
                    ScoreFormat::Time => "0:00.00",
                    ScoreFormat::Score | ScoreFormat::Value => "0",
                };
                (board.title.clone(), zero.to_owned())
            })
        };
        scored.or_else(first).into_iter().collect()
    }

    pub fn follow_game(&mut self, game: Option<&GameState>) {
        let id = game.map(|game| game.id);
        if self.game == id {
            return;
        }
        let keep = (self.view, self.scores_sent);
        *self = Self::default();
        (self.view, self.scores_sent) = keep;
        self.game = id;
        self.list = game
            .iter()
            .flat_map(|game| &game.leaderboards)
            .map(|board| Board {
                id: board.id,
                title: board.title.clone(),
                description: board.description.clone(),
                format: board.format,
                best: Best::Unknown,
            })
            .collect();
    }

    pub fn want_best(&mut self, id: u32) {
        if self.asked_best.insert(id) {
            self.wants.push((id, Fetch::AroundMe, 1));
        }
    }

    pub fn rankings_state(&mut self) -> Option<RankingsState> {
        let id = self.open?;
        if self.asked_rankings != Some(id) {
            self.asked_rankings = Some(id);
            self.rankings_failed = false;
            self.rankings = None;
            self.wants.push((id, Fetch::Top, TOP));
            self.wants.push((id, Fetch::AroundMe, AROUND));
        }
        Some(if self.rankings_failed {
            RankingsState::Failed
        } else if self.rankings.as_ref().is_some_and(|r| r.board == id) {
            RankingsState::Shown
        } else {
            RankingsState::Loading
        })
    }

    pub fn retry_rankings(&mut self) {
        self.asked_rankings = None;
    }

    pub fn take_wants(&mut self) -> Vec<(u32, Fetch, u32)> {
        std::mem::take(&mut self.wants)
    }

    pub fn take_entries(&mut self, entries: Entries) {
        let mine = entries.mine.and_then(|at| entries.entries.get(at)).cloned();
        if let Some(board) = self.board_mut(entries.board) {
            match &mine {
                Some(entry) => board.best = Best::Score(entry.clone()),
                None if entries.fetch == Fetch::AroundMe => board.best = Best::NoScore,
                None => {}
            }
        }
        if self.asked_rankings != Some(entries.board) {
            return;
        }
        let rankings = self.rankings.get_or_insert_with(|| Rankings {
            board: entries.board,
            entries: entries.total,
            top: Vec::new(),
            around: Vec::new(),
        });
        rankings.entries = entries.total;
        match entries.fetch {
            Fetch::Top => rankings.top = entries.entries,
            Fetch::AroundMe if mine.is_some() => rankings.around = entries.entries,
            Fetch::AroundMe => rankings.around.clear(),
        }
        let last_top = rankings.top.last().map(|entry| entry.rank);
        if let Some(last) = last_top {
            rankings.around.retain(|entry| entry.rank > last);
        }
    }

    pub fn fetch_failed(&mut self, board: u32) {
        if self.asked_rankings == Some(board) {
            self.rankings_failed = true;
        }
    }

    pub fn take_scoreboard(&mut self, scoreboard: &Scoreboard, me: &str) {
        if let Some(board) = self.board_mut(scoreboard.board) {
            board.best = Best::Score(Entry {
                rank: scoreboard.rank,
                user: me.to_owned(),
                score: scoreboard.best.clone(),
            });
        }
        if self.asked_rankings == Some(scoreboard.board) {
            self.asked_rankings = None;
        }
    }
}

pub fn rank_line(rank: u32, entries: u32) -> String {
    format!(
        "Rank {} of {}",
        crate::app::tabs::achievements::count(rank),
        crate::app::tabs::achievements::count(entries)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_achievements::{Leaderboard, ScoreFormat, Summary};

    fn game(id: u32) -> GameState {
        let board = |id: u32, title: &str| Leaderboard {
            id,
            title: title.to_owned(),
            description: format!("{title} as fast as you can"),
            format: ScoreFormat::Time,
            lower_is_better: true,
            tracking: false,
        };
        GameState {
            id,
            title: "Pokémon Red Version".to_owned(),
            hash: String::new(),
            badge_url: None,
            summary: Summary::default(),
            groups: Vec::new(),
            leaderboards: vec![board(1, "Brock Speedrun"), board(2, "Elite Four Speedrun")],
        }
    }

    fn attempt(board: u32, score: &str) -> Attempt {
        let title = if board == 1 {
            "Brock Speedrun"
        } else {
            "Elite Four Speedrun"
        };
        Attempt {
            board,
            title: title.to_owned(),
            score: score.to_owned(),
        }
    }

    fn entry(rank: u32, user: &str) -> Entry {
        Entry {
            rank,
            user: user.to_owned(),
            score: format!("0:{rank:02}:00"),
        }
    }

    fn entries(board: u32, fetch: Fetch, ranks: &[(u32, &str)], mine: Option<usize>) -> Entries {
        Entries {
            board,
            fetch,
            total: 3402,
            entries: ranks
                .iter()
                .map(|&(rank, user)| entry(rank, user))
                .collect(),
            mine,
        }
    }

    #[test]
    fn a_new_game_lists_its_boards_and_forgets_the_old_ones() {
        let mut boards = Boards {
            view: View::Leaderboards,
            ..Boards::default()
        };
        boards.follow_game(Some(&game(7)));
        boards.attempts = vec![attempt(1, "0:12")];
        boards.open = Some(1);
        assert_eq!(boards.list.len(), 2);
        assert_eq!(boards.list[0].best, Best::Unknown);

        boards.follow_game(Some(&game(7)));
        assert_eq!(boards.open, Some(1), "the same game keeps the open board");

        boards.follow_game(Some(&game(8)));
        assert_eq!(boards.open, None);
        assert!(boards.attempts.is_empty());
        assert_eq!(boards.view, View::Leaderboards);

        boards.follow_game(None);
        assert!(boards.list.is_empty());
    }

    #[test]
    fn a_shown_row_asks_for_its_best_once() {
        let mut boards = Boards::default();
        boards.follow_game(Some(&game(7)));
        boards.want_best(1);
        boards.want_best(1);
        assert_eq!(boards.take_wants(), [(1, Fetch::AroundMe, 1)]);

        boards.take_entries(entries(1, Fetch::AroundMe, &[(212, "ClaraPlays")], Some(0)));
        assert_eq!(boards.list[0].best, Best::Score(entry(212, "ClaraPlays")));

        boards.want_best(2);
        boards.take_entries(entries(2, Fetch::AroundMe, &[(1, "Zaphod42")], None));
        assert_eq!(boards.list[1].best, Best::NoScore);
    }

    #[test]
    fn opening_a_board_asks_for_the_top_and_the_rows_around_the_player() {
        let mut boards = Boards::default();
        boards.follow_game(Some(&game(7)));
        boards.open = Some(1);
        assert_eq!(boards.rankings_state(), Some(RankingsState::Loading));
        assert_eq!(boards.rankings_state(), Some(RankingsState::Loading));
        assert_eq!(
            boards.take_wants(),
            [(1, Fetch::Top, TOP), (1, Fetch::AroundMe, AROUND)]
        );

        boards.take_entries(entries(
            1,
            Fetch::Top,
            &[(1, "Zaphod42"), (2, "PikaRush")],
            None,
        ));
        boards.take_entries(entries(
            1,
            Fetch::AroundMe,
            &[(2, "PikaRush"), (211, "Kaito"), (212, "ClaraPlays")],
            Some(2),
        ));
        assert_eq!(boards.rankings_state(), Some(RankingsState::Shown));
        let rankings = boards.rankings.as_ref().unwrap();
        assert_eq!(rankings.entries, 3402);
        assert_eq!(rankings.top.len(), 2);
        assert_eq!(
            rankings.around,
            [entry(211, "Kaito"), entry(212, "ClaraPlays")],
            "rows already in the top aren't shown twice"
        );
    }

    #[test]
    fn a_player_in_the_top_has_no_rows_around_them() {
        let mut boards = Boards::default();
        boards.follow_game(Some(&game(7)));
        boards.open = Some(1);
        boards.rankings_state();
        boards.take_entries(entries(
            1,
            Fetch::AroundMe,
            &[(1, "Zaphod42"), (2, "ClaraPlays")],
            Some(1),
        ));
        boards.take_entries(entries(
            1,
            Fetch::Top,
            &[(1, "Zaphod42"), (2, "ClaraPlays")],
            Some(1),
        ));
        assert!(boards.rankings.as_ref().unwrap().around.is_empty());
    }

    #[test]
    fn a_failed_fetch_says_so_and_try_again_asks_again() {
        let mut boards = Boards::default();
        boards.follow_game(Some(&game(7)));
        boards.open = Some(1);
        boards.rankings_state();
        boards.take_wants();
        boards.fetch_failed(1);
        assert_eq!(boards.rankings_state(), Some(RankingsState::Failed));
        boards.retry_rankings();
        assert_eq!(boards.rankings_state(), Some(RankingsState::Loading));
        assert_eq!(boards.take_wants().len(), 2);
    }

    #[test]
    fn a_submitted_score_becomes_the_best_and_refreshes_the_open_rankings() {
        let mut boards = Boards::default();
        boards.follow_game(Some(&game(7)));
        boards.open = Some(1);
        boards.rankings_state();
        boards.take_wants();
        let scoreboard = Scoreboard {
            board: 1,
            submitted: "0:38:12".to_owned(),
            best: "0:38:12".to_owned(),
            rank: 9,
            entries: 3402,
            top: Vec::new(),
        };
        boards.take_scoreboard(&scoreboard, "ClaraPlays");
        assert_eq!(
            boards.list[0].best,
            Best::Score(Entry {
                rank: 9,
                user: "ClaraPlays".to_owned(),
                score: "0:38:12".to_owned()
            })
        );
        boards.rankings_state();
        assert_eq!(boards.take_wants().len(), 2);
    }

    #[test]
    fn softcore_says_leaderboards_need_hardcore() {
        let mut boards = Boards::default();
        assert_eq!(boards.note(), Some(NEEDS_HARDCORE));
        boards.hardcore = true;
        assert_eq!(boards.note(), Some(NOT_SENT));
        boards.scores_sent = true;
        assert_eq!(boards.note(), None);
    }

    #[test]
    fn edit_layout_shows_a_sample_tracker_only_in_hardcore() {
        let mut boards = Boards::default();
        boards.follow_game(Some(&game(7)));
        assert!(boards.layout_lines().is_empty());
        boards.hardcore = true;
        assert_eq!(
            boards.layout_lines(),
            [("Brock Speedrun".to_owned(), "0:00.00".to_owned())]
        );
        boards.list[1].best = Best::Score(entry(4, "ClaraPlays"));
        assert_eq!(
            boards.layout_lines(),
            [("Elite Four Speedrun".to_owned(), "0:04:00".to_owned())]
        );
        boards.attempts = vec![attempt(1, "0:02")];
        assert_eq!(boards.layout_lines(), boards.tracker_lines());
    }

    #[test]
    fn each_running_attempt_is_a_tracker_line() {
        let mut boards = Boards::default();
        boards.follow_game(Some(&game(7)));
        boards.attempts = vec![attempt(1, "0:02"), attempt(2, "1:10")];
        assert_eq!(
            boards.tracker_lines(),
            [
                ("Brock Speedrun".to_owned(), "0:02".to_owned()),
                ("Elite Four Speedrun".to_owned(), "1:10".to_owned())
            ]
        );
    }
}
