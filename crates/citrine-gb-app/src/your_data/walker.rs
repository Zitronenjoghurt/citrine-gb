use super::{Change, Files, Kind, Part, Tally};
use crate::icons;
use crate::walker::{MAIN, Pokewalker, decode, is_walker_file};
use std::io;

const WALKER: &str = "walker/";

pub struct WalkerPart<'a>(pub &'a mut Pokewalker);

impl WalkerPart<'_> {
    fn incoming<'f>(&self, files: &'f Files) -> Vec<(&'f str, &'f [u8], bool)> {
        let mine = self.0.files();
        files
            .range(WALKER.to_owned()..)
            .map_while(|(path, bytes)| Some((path.strip_prefix(WALKER)?, bytes.as_slice())))
            .filter(|(name, bytes)| {
                is_walker_file(name) && (*name != MAIN || decode(bytes).is_some())
            })
            .filter_map(
                |(name, bytes)| match mine.iter().find(|(file, _)| file == name) {
                    None => Some((name, bytes, false)),
                    Some((_, known)) if name == MAIN && known != bytes => Some((name, bytes, true)),
                    Some(_) => None,
                },
            )
            .collect()
    }
}

impl Part for WalkerPart<'_> {
    fn kind(&self) -> Kind {
        Kind {
            name: "Pokéwalker",
            icon: icons::PERSON_SIMPLE_WALK,
            units: ("", ""),
            nouns: ("Pokéwalker", "Pokéwalkers"),
        }
    }

    fn tally(&mut self) -> Tally {
        let files = self.0.files();
        Tally {
            count: files.len(),
            bytes: files.iter().map(|(_, bytes)| bytes.len() as u64).sum(),
        }
    }

    fn export(&mut self, out: &mut Files) {
        for (name, bytes) in self.0.files() {
            out.insert(format!("{WALKER}{name}"), bytes);
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        let incoming = self.incoming(files);
        let replaced = incoming.iter().filter(|(_, _, mine)| *mine).count();
        Change {
            added: incoming.len() - replaced,
            replaced,
        }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        for (name, bytes, _) in self.incoming(files) {
            self.0.restore(name, bytes)?;
        }
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        self.0.forget()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::walker::FileStore;
    use citrine_gb_walker::Button;

    fn pokewalker(dir: &tempfile::TempDir) -> Pokewalker {
        Pokewalker::new(Box::new(FileStore::at(dir.path().to_path_buf())))
    }

    #[test]
    fn importing_another_walker_keeps_this_one_as_a_copy() {
        let (here, there) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut mine = pokewalker(&here);
        mine.press(Button::Middle);
        let mut theirs = pokewalker(&there);
        theirs.walker().unwrap().steps(500);
        theirs.press(Button::Middle);
        let mut file = Files::new();
        WalkerPart(&mut theirs).export(&mut file);
        let ours = mine.files();

        let mut part = WalkerPart(&mut mine);
        assert_eq!(
            part.plan(&file),
            Change {
                added: 0,
                replaced: 1
            }
        );
        part.import(&file).unwrap();
        assert_eq!(part.plan(&file), Change::default());
        let files = mine.files();
        assert_eq!(
            files.len(),
            2,
            "{:?}",
            files.iter().map(|(name, _)| name).collect::<Vec<_>>()
        );
        assert!(
            files
                .iter()
                .any(|(name, bytes)| name != MAIN && *bytes == ours[0].1)
        );
        assert_eq!(
            mine.walker().unwrap().eeprom(),
            theirs.walker().unwrap().eeprom()
        );
    }

    #[test]
    fn delete_leaves_a_new_walker_and_no_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut walker = pokewalker(&dir);
        walker.walker().unwrap().steps(1_000);
        walker.press(Button::Middle);
        let before = walker.walker().unwrap().clone();
        WalkerPart(&mut walker).delete_all().unwrap();
        assert!(walker.files().is_empty());
        assert!(!dir.path().join("walker").exists());
        assert_ne!(walker.walker().unwrap(), &before);
    }
}
