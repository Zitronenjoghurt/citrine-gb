use super::SaveType;

const GAME_CODE: usize = 0xAC;
const OVERRIDES: &[([u8; 4], SaveType)] = &[];
const LIBRARIES: [(&[u8], SaveType); 6] = [
    (b"FLASH1M_V", SaveType::Flash128K),
    (b"FLASH512_V", SaveType::Flash64K),
    (b"FLASH_V", SaveType::Flash64K),
    (b"SRAM_F_V", SaveType::Sram32K),
    (b"SRAM_V", SaveType::Sram32K),
    (b"EEPROM_V", SaveType::Eeprom),
];

impl SaveType {
    pub fn detect(rom: &[u8]) -> Self {
        by_game_code(rom)
            .or_else(|| by_library(rom))
            .unwrap_or(Self::Auto)
    }
}

fn by_game_code(rom: &[u8]) -> Option<SaveType> {
    let code = rom.get(GAME_CODE..GAME_CODE + 4)?;
    OVERRIDES
        .iter()
        .find(|(game, _)| game.as_slice() == code)
        .map(|&(_, kind)| kind)
}

fn by_library(rom: &[u8]) -> Option<SaveType> {
    let mut best: Option<usize> = None;
    for offset in (0..rom.len()).step_by(4) {
        if !matches!(rom[offset], b'F' | b'S' | b'E') {
            continue;
        }
        let tail = &rom[offset..];
        if let Some(rank) = LIBRARIES
            .iter()
            .position(|(name, _)| tail.starts_with(name))
        {
            best = Some(best.map_or(rank, |best| best.min(rank)));
            if rank == 0 {
                break;
            }
        }
    }
    best.map(|rank| LIBRARIES[rank].1)
}
