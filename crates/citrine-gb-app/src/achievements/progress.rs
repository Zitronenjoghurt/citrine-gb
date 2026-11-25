use crate::app::widgets::game_tile::Progress;
use crate::storage::Folders;
use crate::storage::kinds::Folder;
use citrine_gb_achievements::{Console, GameProgress, GameState, SetInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const NAMESPACE: Folder = Folder::AchievementLists;
const FRESH_FOR: u64 = 24 * 60 * 60;
pub const CONSOLES: [Console; 4] = [
    Console::GameBoy,
    Console::GameBoyColor,
    Console::GameBoyAdvance,
    Console::NintendoDs,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Set {
    game_id: u32,
    total: u32,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    fetched: u64,
    sets: Vec<(String, u32, u32)>,
}

#[derive(Default)]
pub struct LibraryProgress {
    sets: HashMap<String, Set>,
    earned: HashMap<u32, u32>,
    pub asked: bool,
}

impl LibraryProgress {
    pub fn of(&self, hash: &str) -> Option<Progress> {
        let set = self.sets.get(&hash.to_ascii_lowercase())?;
        let earned = self.earned.get(&set.game_id).copied().unwrap_or(0);
        Some(Progress {
            earned: earned.min(set.total),
            total: set.total,
        })
    }

    pub fn load_cached(&mut self, cache: &Folders, console: Console, now: u64) -> bool {
        let cached = cache
            .read(NAMESPACE, &file_name(console))
            .and_then(|data| serde_json::from_slice::<Cached>(&data).ok())
            .filter(|cached| now.saturating_sub(cached.fetched) < FRESH_FOR);
        let Some(cached) = cached else {
            return false;
        };
        for (hash, game_id, total) in cached.sets {
            self.sets.insert(hash, Set { game_id, total });
        }
        true
    }

    pub fn take_sets(&mut self, cache: &Folders, console: Console, sets: Vec<SetInfo>, now: u64) {
        let mut cached = Vec::new();
        for set in sets.into_iter().filter(|set| set.total > 0) {
            for hash in set.hashes {
                let hash = hash.to_ascii_lowercase();
                cached.push((hash.clone(), set.game_id, set.total));
                self.sets.insert(
                    hash,
                    Set {
                        game_id: set.game_id,
                        total: set.total,
                    },
                );
            }
        }
        let cached = Cached {
            fetched: now,
            sets: cached,
        };
        if let Ok(data) = serde_json::to_vec(&cached)
            && let Err(error) = cache.write(NAMESPACE, &file_name(console), &data)
        {
            log::warn!("achievements: the set list wasn't cached: {error}");
        }
    }

    pub fn take_progress(&mut self, games: Vec<GameProgress>) {
        for game in games {
            let earned = game.unlocked.max(game.unlocked_hardcore);
            self.earned.insert(game.game_id, earned);
        }
    }

    pub fn saw(&mut self, game: &GameState) {
        if game.summary.total == 0 {
            return;
        }
        self.sets.insert(
            game.hash.to_ascii_lowercase(),
            Set {
                game_id: game.id,
                total: game.summary.total,
            },
        );
        self.earned.insert(game.id, game.summary.unlocked);
    }
}

fn file_name(console: Console) -> String {
    format!("sets-{}.json", console.id())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn hash(console: Option<citrine_gb::console::ConsoleKind>, rom: &[u8]) -> Option<String> {
    use citrine_gb::console::ConsoleKind;
    let console = match console? {
        ConsoleKind::Gb | ConsoleKind::Gbc => Console::GameBoy,
        ConsoleKind::Gba => Console::GameBoyAdvance,
        ConsoleKind::Nds => Console::NintendoDs,
    };
    citrine_gb_achievements::hash_rom(console, rom).map(|hash| hash.to_ascii_lowercase())
}

pub fn wants_hash(console: Option<citrine_gb::console::ConsoleKind>) -> bool {
    cfg!(not(target_arch = "wasm32")) && console.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_achievements::Summary;

    fn red() -> SetInfo {
        SetInfo {
            game_id: 724,
            title: "Pokemon Red Version".to_owned(),
            total: 52,
            points: 400,
            hashes: vec!["3D45C1EE9ABD5738DF46D2BDDA8B57DC".to_owned()],
        }
    }

    fn cache(name: &str) -> Folders {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-ra-progress-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Folders::at(root)
    }

    #[test]
    fn a_game_shows_its_earned_count_of_its_set() {
        let mut progress = LibraryProgress::default();
        progress.take_sets(&cache("count"), Console::GameBoy, vec![red()], 1000);
        assert_eq!(
            progress.of("3d45c1ee9abd5738df46d2bdda8b57dc"),
            Some(Progress {
                earned: 0,
                total: 52
            })
        );
        progress.take_progress(vec![GameProgress {
            game_id: 724,
            total: 52,
            unlocked: 9,
            unlocked_hardcore: 12,
        }]);
        assert_eq!(
            progress.of("3D45C1EE9ABD5738DF46D2BDDA8B57DC"),
            Some(Progress {
                earned: 12,
                total: 52
            })
        );
        assert_eq!(progress.of("00000000000000000000000000000000"), None);
    }

    #[test]
    fn a_set_list_stays_cached_for_a_day() {
        let cache = cache("day");
        LibraryProgress::default().take_sets(&cache, Console::GameBoy, vec![red()], 1000);
        let mut fresh = LibraryProgress::default();
        assert!(fresh.load_cached(&cache, Console::GameBoy, 1000 + 23 * 3600));
        assert!(fresh.of("3d45c1ee9abd5738df46d2bdda8b57dc").is_some());
        assert!(!fresh.load_cached(&cache, Console::GameBoyColor, 1000));
        let mut stale = LibraryProgress::default();
        assert!(!stale.load_cached(&cache, Console::GameBoy, 1000 + 25 * 3600));
        assert!(stale.of("3d45c1ee9abd5738df46d2bdda8b57dc").is_none());
    }

    #[test]
    fn the_running_game_counts_its_unlocks_at_once() {
        let mut progress = LibraryProgress::default();
        progress.take_sets(&cache("live"), Console::GameBoy, vec![red()], 1000);
        progress.saw(&GameState {
            id: 724,
            title: "Pokemon Red Version".to_owned(),
            hash: "3d45c1ee9abd5738df46d2bdda8b57dc".to_owned(),
            badge_url: None,
            summary: Summary {
                total: 52,
                unlocked: 3,
                points_total: 400,
                points_unlocked: 20,
            },
            groups: Vec::new(),
            leaderboards: Vec::new(),
        });
        assert_eq!(
            progress.of("3d45c1ee9abd5738df46d2bdda8b57dc"),
            Some(Progress {
                earned: 3,
                total: 52
            })
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_game_boy_or_gba_rom_hashes_to_its_md5() {
        use citrine_gb::console::ConsoleKind;
        let rom = [0u8; 0x8000];
        assert_eq!(
            hash(Some(ConsoleKind::Gb), &rom).as_deref(),
            Some("bb7df04e1b0a2570657527a7e108ae23")
        );
        assert_eq!(
            hash(Some(ConsoleKind::Gba), &rom).as_deref(),
            Some("bb7df04e1b0a2570657527a7e108ae23")
        );
    }
}
