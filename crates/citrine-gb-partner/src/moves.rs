use citrine_gb_gamedata::{Game, Generation};

const CHECKED: usize = 16;
const PP_OFFSET: usize = 5;
const KNOWN_PP: [u8; 3] = [35, 25, 10];

pub(crate) fn pp_table(game: Game, rom: &[u8]) -> Option<Vec<u8>> {
    let (record, count) = match game.generation() {
        Generation::One => (6, 165),
        Generation::Two => (7, 251),
        Generation::Three | Generation::Four | Generation::Five => return None,
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

#[cfg(test)]
mod tests {
    use crate::moves::pp_table;
    use crate::test_support::game;
    use citrine_gb_gamedata::{Language, Version};

    #[test]
    fn the_move_table_is_found_by_its_first_records() {
        let mut rom = vec![0u8; 0x4000];
        for (record, generation) in [(6, Version::Red), (7, Version::Gold)] {
            let at = 0x1000;
            let count = if record == 6 { 165 } else { 251 };
            for i in 0..count {
                rom[at + i * record] = i as u8 + 1;
                rom[at + i * record + 5] = [35, 25, 10][i % 3];
            }
            let pp = pp_table(game(generation, Language::English), &rom).expect("a table");
            assert_eq!(pp.len(), count + 1);
            assert_eq!(&pp[..4], &[0, 35, 25, 10]);
            rom.fill(0);
        }
    }
}
