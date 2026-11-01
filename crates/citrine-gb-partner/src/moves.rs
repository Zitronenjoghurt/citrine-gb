use citrine_gb_gamedata::{Game, Generation};

const CHECKED: usize = 16;
const PP_OFFSET: usize = 5;
const KNOWN_PP: [u8; 3] = [35, 25, 10];

pub(crate) fn pp_table(game: Game, rom: &[u8]) -> Option<Vec<u8>> {
    let (record, count) = match game.generation() {
        Generation::One => (6, 165),
        Generation::Two => (7, 251),
    };
    let span = record * count;
    let at = (0..rom.len().saturating_sub(span)).find(|&at| {
        (0..CHECKED).all(|i| usize::from(rom[at + i * record]) == i + 1)
            && KNOWN_PP
                .iter()
                .enumerate()
                .all(|(i, &pp)| rom[at + i * record + PP_OFFSET] == pp)
    })?;
    let mut pp = vec![0];
    pp.extend(
        rom[at..at + span]
            .chunks_exact(record)
            .map(|r| r[PP_OFFSET]),
    );
    Some(pp)
}
