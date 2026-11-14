use super::{ExtractError, Game, u16_at, u32_at};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Place {
    pub header: u16,
    pub encounters: Option<u16>,
    pub location: u16,
    pub area_icon: u8,
    pub battle_background: u8,
}

const HEADER_LEN: usize = 24;
const MAP_NAME_LEN: usize = 16;
const ZONE_LEN: usize = 48;
const ZONE_NAME: usize = 0x1A;

pub(super) fn header_table(code: [u8; 4]) -> Option<usize> {
    match &code {
        b"CPUE" => Some(0xE601C),
        b"CPUD" => Some(0xE6074),
        b"IPKE" | b"IPGE" => Some(0xF6BE0),
        b"IPKD" | b"IPGD" => Some(0xF6B94),
        _ => None,
    }
}

pub(super) fn read(
    game: Game,
    arm9: &[u8],
    table: usize,
    map_names: &[u8],
) -> Result<Vec<Place>, ExtractError> {
    let count = map_names.len() / MAP_NAME_LEN;
    (0..count)
        .map(|header| {
            let at = table + header * HEADER_LEN;
            let r = arm9
                .get(at..at + HEADER_LEN)
                .ok_or(ExtractError::ShortRecord {
                    what: "map header",
                    index: header,
                    len: arm9.len().saturating_sub(at),
                })?;
            let (encounters, icon, background) = match game {
                Game::HeartGold | Game::SoulSilver => (
                    (r[0] != 0xFF).then_some(u16::from(r[0])),
                    r[19] & 0x0F,
                    (u32_at(r, 20) >> 20 & 0x1F) as u8,
                ),
                _ => {
                    let file = u16_at(r, 14);
                    (
                        (file != 0xFFFF).then_some(file),
                        r[19],
                        (u16_at(r, 22) >> 7 & 0x1F) as u8,
                    )
                }
            };
            Ok(Place {
                header: header as u16,
                encounters,
                location: u16::from(r[18]),
                area_icon: icon,
                battle_background: background,
            })
        })
        .collect()
}

pub(super) fn read_zones(zones: &[u8]) -> Vec<Place> {
    zones
        .as_chunks::<ZONE_LEN>()
        .0
        .iter()
        .enumerate()
        .map(|(zone, r)| Place {
            header: zone as u16,
            encounters: None,
            location: u16_at(r, ZONE_NAME),
            area_icon: 0,
            battle_background: 0,
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn zone_record(location: u16) -> Vec<u8> {
    let mut r = vec![0u8; ZONE_LEN];
    r[ZONE_NAME..ZONE_NAME + 2].copy_from_slice(&location.to_le_bytes());
    r
}

#[cfg(test)]
pub(crate) fn header_record(game: Game, place: &Place) -> Vec<u8> {
    let mut r = vec![0u8; HEADER_LEN];
    match game {
        Game::HeartGold | Game::SoulSilver => {
            r[0] = place.encounters.map_or(0xFF, |f| f as u8);
            r[19] = 0x90 | place.area_icon;
            let flags = 0x8000_0001 | u32::from(place.battle_background) << 20;
            r[20..24].copy_from_slice(&flags.to_le_bytes());
        }
        _ => {
            r[14..16].copy_from_slice(&place.encounters.unwrap_or(0xFFFF).to_le_bytes());
            r[19] = place.area_icon;
            let flags = 0x7005 | u16::from(place.battle_background) << 7;
            r[22..24].copy_from_slice(&flags.to_le_bytes());
        }
    }
    r[18] = place.location as u8;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_header_named_in_the_map_name_list() {
        for game in [Game::Platinum, Game::SoulSilver] {
            let places = [
                Place {
                    header: 0,
                    encounters: None,
                    location: 0,
                    area_icon: 0,
                    battle_background: 0,
                },
                Place {
                    header: 1,
                    encounters: Some(23),
                    location: 52,
                    area_icon: 5,
                    battle_background: 3,
                },
            ];
            let mut arm9 = vec![0xAA; 0x40];
            for p in &places {
                arm9.extend(header_record(game, p));
            }
            arm9.extend(header_record(game, &places[1]));
            let read = read(game, &arm9, 0x40, &[0; 32]).unwrap();
            assert_eq!(read, places, "{game:?}");
        }
    }

    #[test]
    fn reads_the_place_name_of_each_gen_5_zone() {
        let zones = [zone_record(4), zone_record(81), vec![0; 20]].concat();
        let read = read_zones(&zones);
        assert_eq!(read.len(), 2);
        assert_eq!((read[1].header, read[1].location), (1, 81));
    }

    #[test]
    fn a_table_past_the_code_is_an_error() {
        assert!(read(Game::Platinum, &[0; 30], 0, &[0; 32]).is_err());
    }
}
