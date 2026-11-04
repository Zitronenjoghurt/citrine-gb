use crate::homebrew::HOMEBREW_GAMES;
use citrine_gb_net::GameHash;

pub trait Shelf {
    fn find(&self, hash: &GameHash) -> Option<Vec<u8>>;
}

pub struct Homebrew;

impl Shelf for Homebrew {
    fn find(&self, hash: &GameHash) -> Option<Vec<u8>> {
        HOMEBREW_GAMES
            .iter()
            .find(|game| game.sha1 == *hash)
            .and_then(|game| game.embedded())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Files {
    pub paths: Vec<std::path::PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Shelf for Files {
    fn find(&self, hash: &GameHash) -> Option<Vec<u8>> {
        Homebrew.find(hash).or_else(|| {
            self.paths.iter().find_map(|path| {
                let size = std::fs::metadata(path).ok()?.len();
                if size > citrine_gb_net::MAX_GAME_BYTES as u64 {
                    return None;
                }
                let data = std::fs::read(path).ok()?;
                (super::sha1_of(&data) == Some(*hash)).then_some(data)
            })
        })
    }
}
