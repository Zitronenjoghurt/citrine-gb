use crate::id::{ALPHABET, IdError, symbols};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

const INVITE_CHARS: usize = 12;
const INVITE_GROUP: usize = 4;
const INVITE_BITS: u32 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InviteCode(u64);

impl InviteCode {
    pub const MAX: u64 = (1 << INVITE_BITS) - 1;

    pub fn new(value: u64) -> Option<Self> {
        (value <= Self::MAX).then_some(Self(value))
    }

    pub fn random() -> Self {
        let mut bytes = [0; 8];
        getrandom::getrandom(&mut bytes).expect("no system random source");
        Self(u64::from_be_bytes(bytes) & Self::MAX)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for InviteCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in 0..INVITE_CHARS {
            if i > 0 && i % INVITE_GROUP == 0 {
                f.write_str("-")?;
            }
            let shift = INVITE_BITS as usize - 5 * (i + 1);
            let symbol = ALPHABET[((self.0 >> shift) & 31) as usize] as char;
            write!(f, "{symbol}")?;
        }
        Ok(())
    }
}

impl FromStr for InviteCode {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = symbols(input, INVITE_CHARS)?
            .into_iter()
            .fold(0u64, |acc, s| (acc << 5) | s as u64);
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_and_parse_round_trip() {
        for _ in 0..64 {
            let code = InviteCode::random();
            assert!(code.value() <= InviteCode::MAX);
            assert_eq!(code.to_string().parse::<InviteCode>(), Ok(code));
        }
        let max = InviteCode::new(InviteCode::MAX).unwrap();
        assert_eq!(max.to_string(), "ZZZZ-ZZZZ-ZZZZ");
        assert_eq!(InviteCode::new(0).unwrap().to_string(), "0000-0000-0000");
        assert_eq!(InviteCode::new(InviteCode::MAX + 1), None);
    }

    #[test]
    fn reads_like_an_online_id() {
        let text = InviteCode::new(0x0123_4567_89AB_CDEF & InviteCode::MAX)
            .unwrap()
            .to_string();
        let groups: Vec<&str> = text.split('-').collect();
        assert_eq!(groups.len(), 3);
        assert!(groups.iter().all(|g| g.len() == 4));
        assert!(!text.contains(['I', 'L', 'O', 'U']));
    }

    #[test]
    fn typing_is_forgiving_about_case_dashes_spaces_and_lookalikes() {
        let code: InviteCode = "7KQ2-M9XD-4HT0".parse().unwrap();
        assert_eq!("7kq2m9xd4ht0".parse(), Ok(code));
        assert_eq!(" 7kq2 m9xd 4hto ".parse(), Ok(code));
        assert_eq!("7KQ2-M9XD-4HTO".parse(), Ok(code));
        let one: InviteCode = "1111-1111-1111".parse().unwrap();
        assert_eq!("iIlL-1111-1111".parse(), Ok(one));
    }

    #[test]
    fn parse_errors() {
        assert_eq!(
            "7KQ2-M9XD".parse::<InviteCode>(),
            Err(IdError::Length {
                found: 8,
                expected: 12
            })
        );
        assert_eq!(
            "7KQ2-M9XD-4HTU".parse::<InviteCode>(),
            Err(IdError::Char('U'))
        );
        assert!("".parse::<InviteCode>().is_err());
    }
}
