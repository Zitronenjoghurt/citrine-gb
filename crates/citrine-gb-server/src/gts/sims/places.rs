use citrine_gb_pkm::Language;
use citrine_gb_pkm::generate::Rng;

const ARGENTINA: u8 = 9;
const AUSTRALIA: u8 = 12;
const AUSTRIA: u8 = 13;
const BELGIUM: u8 = 20;
const CANADA: u8 = 36;
const FRANCE: u8 = 71;
const GERMANY: u8 = 77;
const ITALY: u8 = 101;
const JAPAN: u8 = 103;
const KOREA: u8 = 110;
const MEXICO: u8 = 135;
const SPAIN: u8 = 193;
const SWITZERLAND: u8 = 200;
const UK: u8 = 219;
const USA: u8 = 220;

const EASTERN: i8 = -5;
const CENTRAL: i8 = -6;
const MOUNTAIN: i8 = -7;
const PACIFIC: i8 = -8;

const MALE_AVATARS: &[u8] = &[0];
const FEMALE_AVATARS: &[u8] = &[1];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub country: u8,
    pub region: u8,
    pub utc_offset: i8,
}

enum Regions {
    None(i8),
    Even(u8, i8),
    Weighted(&'static [(u8, i8, u64)]),
}

struct Country {
    country: u8,
    weight: u64,
    regions: Regions,
}

const fn country(country: u8, weight: u64, regions: Regions) -> Country {
    Country {
        country,
        weight,
        regions,
    }
}

const JAPANESE: &[Country] = &[country(JAPAN, 1, Regions::Even(50, 9))];
const ENGLISH: &[Country] = &[
    country(USA, 70, Regions::Weighted(STATES)),
    country(UK, 15, Regions::Even(12, 0)),
    country(CANADA, 8, Regions::Weighted(PROVINCES)),
    country(AUSTRALIA, 7, Regions::Weighted(AUSTRALIAN_STATES)),
];
const FRENCH: &[Country] = &[
    country(FRANCE, 75, Regions::Even(22, 1)),
    country(CANADA, 15, Regions::Weighted(QUEBEC)),
    country(BELGIUM, 10, Regions::None(1)),
];
const GERMAN: &[Country] = &[
    country(GERMANY, 80, Regions::Even(16, 1)),
    country(AUSTRIA, 10, Regions::None(1)),
    country(SWITZERLAND, 10, Regions::None(1)),
];
const SPANISH: &[Country] = &[
    country(SPAIN, 50, Regions::Even(17, 1)),
    country(MEXICO, 35, Regions::None(CENTRAL)),
    country(ARGENTINA, 15, Regions::Even(24, -3)),
];
const ITALIAN: &[Country] = &[country(ITALY, 1, Regions::Even(20, 1))];
const KOREAN: &[Country] = &[country(KOREA, 1, Regions::None(9))];

const PROVINCES: &[(u8, i8, u64)] = &[
    (1, MOUNTAIN, 4),
    (2, PACIFIC, 5),
    (3, CENTRAL, 1),
    (4, -4, 1),
    (5, -3, 1),
    (7, -4, 1),
    (9, EASTERN, 15),
    (11, EASTERN, 2),
    (12, CENTRAL, 1),
];
const QUEBEC: &[(u8, i8, u64)] = &[(11, EASTERN, 1)];
const AUSTRALIAN_STATES: &[(u8, i8, u64)] = &[
    (1, 10, 8),
    (2, 9, 1),
    (3, 10, 5),
    (4, 9, 2),
    (5, 10, 1),
    (6, 10, 7),
    (7, 8, 3),
];

const STATES: &[(u8, i8, u64)] = &[
    (1, CENTRAL, 5),
    (2, -9, 1),
    (3, MOUNTAIN, 7),
    (4, CENTRAL, 3),
    (5, PACIFIC, 39),
    (6, MOUNTAIN, 6),
    (7, EASTERN, 4),
    (8, EASTERN, 1),
    (9, EASTERN, 22),
    (10, EASTERN, 11),
    (11, -10, 1),
    (12, MOUNTAIN, 2),
    (13, CENTRAL, 13),
    (14, EASTERN, 7),
    (15, CENTRAL, 3),
    (16, CENTRAL, 3),
    (17, EASTERN, 4),
    (18, CENTRAL, 5),
    (19, EASTERN, 1),
    (20, EASTERN, 6),
    (21, EASTERN, 7),
    (22, EASTERN, 10),
    (23, CENTRAL, 6),
    (24, CENTRAL, 3),
    (25, CENTRAL, 6),
    (26, MOUNTAIN, 1),
    (27, CENTRAL, 2),
    (28, PACIFIC, 3),
    (29, EASTERN, 1),
    (30, EASTERN, 9),
    (31, MOUNTAIN, 2),
    (32, EASTERN, 20),
    (33, EASTERN, 10),
    (34, CENTRAL, 1),
    (35, EASTERN, 12),
    (36, CENTRAL, 4),
    (37, PACIFIC, 4),
    (38, EASTERN, 13),
    (39, EASTERN, 1),
    (40, EASTERN, 5),
    (41, CENTRAL, 1),
    (42, CENTRAL, 7),
    (43, CENTRAL, 30),
    (44, MOUNTAIN, 3),
    (45, EASTERN, 1),
    (46, EASTERN, 9),
    (47, PACIFIC, 8),
    (48, EASTERN, 1),
    (49, EASTERN, 2),
    (50, CENTRAL, 6),
    (51, MOUNTAIN, 1),
];

fn weighted<T>(rng: &mut Rng, items: &[T], weight: impl Fn(&T) -> u64) -> usize {
    let total: u64 = items.iter().map(&weight).sum();
    let mut roll = rng.below(total.max(1));
    for (i, item) in items.iter().enumerate() {
        let w = weight(item);
        if roll < w {
            return i;
        }
        roll -= w;
    }
    0
}

fn countries(language: Language) -> &'static [Country] {
    match language {
        Language::Japanese => JAPANESE,
        Language::French => FRENCH,
        Language::German => GERMAN,
        Language::Spanish => SPANISH,
        Language::Italian => ITALIAN,
        Language::Korean => KOREAN,
        Language::English | Language::Other(_) => ENGLISH,
    }
}

pub fn pick(rng: &mut Rng, language: Language) -> Place {
    let list = countries(language);
    let home = &list[weighted(rng, list, |c| c.weight)];
    let (region, utc_offset) = match home.regions {
        Regions::None(utc_offset) => (0, utc_offset),
        Regions::Even(count, utc_offset) => (1 + rng.below(u64::from(count)) as u8, utc_offset),
        Regions::Weighted(regions) => {
            let (region, utc_offset, _) = regions[weighted(rng, regions, |r| r.2)];
            (region, utc_offset)
        }
    };
    Place {
        country: home.country,
        region,
        utc_offset,
    }
}

pub fn avatar(rng: &mut Rng, female: bool) -> u8 {
    *rng.pick(if female { FEMALE_AVATARS } else { MALE_AVATARS })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_lands_in_a_country_that_speaks_it() {
        let mut rng = Rng::new(7);
        for language in [
            Language::Japanese,
            Language::English,
            Language::French,
            Language::German,
            Language::Spanish,
            Language::Italian,
            Language::Korean,
        ] {
            for _ in 0..200 {
                let place = pick(&mut rng, language);
                assert!(
                    countries(language)
                        .iter()
                        .any(|c| c.country == place.country)
                );
                assert!(place.region <= 51);
                assert!((-10..=10).contains(&place.utc_offset));
                let has_regions =
                    ![AUSTRIA, BELGIUM, SWITZERLAND, MEXICO, KOREA].contains(&place.country);
                assert_eq!(place.region != 0, has_regions);
            }
        }
    }
}
