use super::{ExtractError, record, u16_at};
use crate::pkm::Gen;
use crate::stats::Stats;
use citrine_gb_nds_files::Narc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BaseStats {
    pub stats: Stats<u8>,
    pub types: [u8; 2],
    pub catch_rate: u8,
    pub base_exp: u16,
    pub ev_yield: Stats<u8>,
    pub gender_ratio: u8,
    pub egg_cycles: u8,
    pub base_friendship: u8,
    pub growth: u8,
    pub egg_groups: [u8; 2],
    pub abilities: [u8; 3],
    pub form_count: u8,
    pub form_index: u16,
}

const GEN4_LEN: usize = 0x1A;
const GEN5_LEN: usize = 0x24;

pub(super) fn read(generation: Gen, narc: &Narc<'_>) -> Result<Vec<BaseStats>, ExtractError> {
    narc.files()
        .enumerate()
        .map(|(index, file)| {
            let file = file?;
            Ok(match generation {
                Gen::Four => gen4(record("personal", index, file, GEN4_LEN)?),
                Gen::Five => gen5(record("personal", index, file, GEN5_LEN)?),
            })
        })
        .collect()
}

fn stats(r: &[u8]) -> Stats<u8> {
    Stats::from_array(std::array::from_fn(|i| r[i]))
}

fn ev_yield(packed: u16) -> Stats<u8> {
    Stats::from_array(std::array::from_fn(|i| ((packed >> (2 * i)) & 3) as u8))
}

fn gen4(r: &[u8]) -> BaseStats {
    BaseStats {
        stats: stats(r),
        types: [r[6], r[7]],
        catch_rate: r[8],
        base_exp: u16::from(r[9]),
        ev_yield: ev_yield(u16_at(r, 0x0A)),
        gender_ratio: r[0x10],
        egg_cycles: r[0x11],
        base_friendship: r[0x12],
        growth: r[0x13],
        egg_groups: [r[0x14], r[0x15]],
        abilities: [r[0x16], r[0x17], 0],
        form_count: 1,
        form_index: 0,
    }
}

fn gen5(r: &[u8]) -> BaseStats {
    BaseStats {
        stats: stats(r),
        types: [r[6], r[7]],
        catch_rate: r[8],
        base_exp: u16_at(r, 0x22),
        ev_yield: ev_yield(u16_at(r, 0x0A)),
        gender_ratio: r[0x12],
        egg_cycles: r[0x13],
        base_friendship: r[0x14],
        growth: r[0x15],
        egg_groups: [r[0x16], r[0x17]],
        abilities: [r[0x18], r[0x19], r[0x1A]],
        form_count: r[0x20],
        form_index: u16_at(r, 0x1C),
    }
}

#[cfg(test)]
pub(crate) fn gen4_record(base: &BaseStats) -> Vec<u8> {
    let mut r = vec![0u8; 0x2C];
    r[..6].copy_from_slice(&base.stats.to_array());
    r[6..8].copy_from_slice(&base.types);
    r[8] = base.catch_rate;
    r[9] = base.base_exp as u8;
    r[0x0A..0x0C].copy_from_slice(&pack_ev_yield(base.ev_yield).to_le_bytes());
    r[0x10] = base.gender_ratio;
    r[0x11] = base.egg_cycles;
    r[0x12] = base.base_friendship;
    r[0x13] = base.growth;
    r[0x14..0x16].copy_from_slice(&base.egg_groups);
    r[0x16..0x18].copy_from_slice(&base.abilities[..2]);
    r
}

#[cfg(test)]
pub(crate) fn gen5_record(base: &BaseStats) -> Vec<u8> {
    let mut r = vec![0u8; 0x4C];
    r[..6].copy_from_slice(&base.stats.to_array());
    r[6..8].copy_from_slice(&base.types);
    r[8] = base.catch_rate;
    r[0x0A..0x0C].copy_from_slice(&pack_ev_yield(base.ev_yield).to_le_bytes());
    r[0x12] = base.gender_ratio;
    r[0x13] = base.egg_cycles;
    r[0x14] = base.base_friendship;
    r[0x15] = base.growth;
    r[0x16..0x18].copy_from_slice(&base.egg_groups);
    r[0x18..0x1B].copy_from_slice(&base.abilities);
    r[0x1C..0x1E].copy_from_slice(&base.form_index.to_le_bytes());
    r[0x20] = base.form_count;
    r[0x22..0x24].copy_from_slice(&base.base_exp.to_le_bytes());
    r
}

#[cfg(test)]
fn pack_ev_yield(evs: Stats<u8>) -> u16 {
    evs.to_array()
        .iter()
        .enumerate()
        .fold(0, |packed, (i, &ev)| packed | u16::from(ev & 3) << (2 * i))
}

#[cfg(test)]
pub(crate) fn turtwig() -> BaseStats {
    BaseStats {
        stats: Stats::from_array([55, 68, 64, 31, 45, 55]),
        types: [12, 12],
        catch_rate: 45,
        base_exp: 64,
        ev_yield: Stats::from_array([0, 1, 0, 0, 0, 0]),
        gender_ratio: 31,
        egg_cycles: 20,
        base_friendship: 70,
        growth: 3,
        egg_groups: [1, 7],
        abilities: [65, 0, 0],
        form_count: 1,
        form_index: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::narc;

    #[test]
    fn reads_gen4_records() {
        let bytes = narc::build(&[vec![0; 0x2C], gen4_record(&turtwig())]);
        let table = read(Gen::Four, &Narc::parse(&bytes).unwrap()).unwrap();
        assert_eq!(table[1], turtwig());
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn reads_gen5_records_with_three_abilities() {
        let mut snivy = turtwig();
        snivy.abilities = [65, 0, 126];
        snivy.base_exp = 28;
        let bytes = narc::build(&[gen5_record(&snivy)]);
        let table = read(Gen::Five, &Narc::parse(&bytes).unwrap()).unwrap();
        assert_eq!(table[0], snivy);
    }

    #[test]
    fn a_short_record_is_an_error() {
        let bytes = narc::build(&[vec![0; 4]]);
        assert!(matches!(
            read(Gen::Four, &Narc::parse(&bytes).unwrap()),
            Err(ExtractError::ShortRecord { index: 0, .. })
        ));
    }
}
