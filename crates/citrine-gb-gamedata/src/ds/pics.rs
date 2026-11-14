use citrine_gb_nds_files::{Narc, Ncgr, Nclr, NdsRom, Picture};
use citrine_gb_pkm::tables::Game;

const GEN4_FILES: usize = 6;
const GEN4_FRONT_FEMALE: usize = 2;
const GEN4_FRONT_MALE: usize = 3;
const GEN4_PALETTE: usize = 4;
const GEN4_SHINY_PALETTE: usize = 5;
const GEN4_SHEET_WIDTH: usize = 160;
const GEN4_HEIGHT: usize = 80;
const GEN4_SIDE: usize = 80;
const SEED_MULTIPLIER: u32 = 0x41C6_4E6D;
const SEED_INCREMENT: u32 = 0x6073;

pub(super) fn front(
    rom: &[u8],
    game: Game,
    species: u16,
    female: bool,
    shiny: bool,
) -> Option<Picture> {
    let path = match game {
        Game::Diamond | Game::Pearl => "poketool/pokegra/pokegra.narc",
        Game::Platinum => "poketool/pokegra/pl_pokegra.narc",
        Game::HeartGold | Game::SoulSilver => "a/0/0/4",
        Game::Black | Game::White | Game::Black2 | Game::White2 => return None,
    };
    let parsed = NdsRom::parse(rom).ok()?;
    let narc = Narc::parse(parsed.file(path).ok()?).ok()?;
    let base = usize::from(species) * GEN4_FILES;
    let sheet = [GEN4_FRONT_FEMALE, GEN4_FRONT_MALE]
        .into_iter()
        .filter(|&side| female || side == GEN4_FRONT_MALE)
        .find_map(|side| narc.file(base + side).ok().filter(|file| !file.is_empty()))?;
    let palette = narc
        .file(
            base + if shiny {
                GEN4_SHINY_PALETTE
            } else {
                GEN4_PALETTE
            },
        )
        .ok()?;
    let palette = Nclr::parse(palette).ok()?;
    let reversed = matches!(game, Game::Diamond | Game::Pearl);
    let indices = decrypt(Ncgr::parse(sheet).ok()?.data, reversed);
    Some(cut(&indices, &palette))
}

fn decrypt(data: &[u8], reversed: bool) -> Vec<u8> {
    let mut words: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let mut order: Vec<usize> = (0..words.len()).collect();
    if reversed {
        order.reverse();
    }
    let mut seed = u32::from(order.first().map_or(0, |&i| words[i]));
    for i in order {
        words[i] ^= seed as u16;
        seed = seed
            .wrapping_mul(SEED_MULTIPLIER)
            .wrapping_add(SEED_INCREMENT);
    }
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn cut(indices: &[u8], palette: &Nclr) -> Picture {
    let mut rgba = vec![0u8; GEN4_SIDE * GEN4_HEIGHT * 4];
    for y in 0..GEN4_HEIGHT {
        for x in 0..GEN4_SIDE {
            let at = y * GEN4_SHEET_WIDTH + x;
            let Some(&byte) = indices.get(at / 2) else {
                continue;
            };
            let index = if at.is_multiple_of(2) {
                byte & 15
            } else {
                byte >> 4
            };
            if index != 0 {
                let out = (y * GEN4_SIDE + x) * 4;
                rgba[out..out + 4].copy_from_slice(&palette.rgba(usize::from(index)));
            }
        }
    }
    Picture {
        width: GEN4_SIDE,
        height: GEN4_HEIGHT,
        rgba,
    }
}
