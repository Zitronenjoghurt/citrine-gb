use super::{ExtractError, record, u32_at};
use citrine_gb_nds_files::Narc;

const LEVELS: usize = 101;
const MOVE_LEN: usize = 16;
const PP_AT: usize = 6;

pub(super) fn read_growth(narc: &Narc<'_>) -> Result<Vec<Vec<u32>>, ExtractError> {
    narc.files()
        .enumerate()
        .map(|(index, file)| {
            let r = record("growth", index, file?, LEVELS * 4)?;
            Ok((0..LEVELS).map(|level| u32_at(r, level * 4)).collect())
        })
        .collect()
}

pub(super) fn read_pp(narc: &Narc<'_>) -> Result<Vec<u8>, ExtractError> {
    narc.files()
        .enumerate()
        .map(|(index, file)| Ok(record("move", index, file?, MOVE_LEN)?[PP_AT]))
        .collect()
}

#[cfg(test)]
pub(crate) fn growth_record(curve: impl Fn(u32) -> u32) -> Vec<u8> {
    (0..LEVELS as u32)
        .flat_map(|l| curve(l).to_le_bytes())
        .collect()
}

#[cfg(test)]
pub(crate) fn move_record(pp: u8) -> Vec<u8> {
    let mut r = vec![0u8; MOVE_LEN];
    r[PP_AT] = pp;
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::narc;

    #[test]
    fn reads_the_experience_for_each_level() {
        let bytes = narc::build(&[growth_record(|l| l * l * l)]);
        let growth = read_growth(&Narc::parse(&bytes).unwrap()).unwrap();
        assert_eq!((growth[0][1], growth[0][100]), (1, 1_000_000));
    }

    #[test]
    fn reads_the_pp_of_each_move() {
        let bytes = narc::build(&[move_record(0), move_record(35), move_record(5)]);
        assert_eq!(read_pp(&Narc::parse(&bytes).unwrap()).unwrap(), [0, 35, 5]);
    }
}
