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
            .map(|game| game.data())
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
                (citrine_gb::rom::Rom::new(&data).header().ok()?.sha1 == *hash).then_some(data)
            })
        })
    }
}
