use crate::crypto::Keys;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

pub(crate) const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const RELAY_PREFIX: &str = "cgb1";
const RELAY_CHECKSUM_BYTES: usize = 3;
const RELAY_CHARS: usize = 56;
const RELAY_GROUP: usize = 8;
const ONLINE_CHARS: usize = 8;
const ONLINE_BITS: u32 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("a relay ID starts with cgb1-")]
    Prefix,
    #[error("'{0}' can't be part of an ID")]
    Char(char),
    #[error("the ID has {found} characters instead of {expected}")]
    Length { found: usize, expected: usize },
    #[error("the ID has a typo")]
    Checksum,
}

fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn symbol(c: char) -> Result<u8, IdError> {
    let upper = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    ALPHABET
        .iter()
        .position(|&a| a as char == upper)
        .map(|p| p as u8)
        .ok_or(IdError::Char(c))
}

pub(crate) fn symbols(input: &str, expected: usize) -> Result<Vec<u8>, IdError> {
    let values = input
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .map(symbol)
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() != expected {
        return Err(IdError::Length {
            found: values.len(),
            expected,
        });
    }
    Ok(values)
}

fn decode(values: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &value in values {
        buffer = (buffer << 5) | value as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

#[derive(Clone, PartialEq, Eq)]
pub struct RelayId([u8; 32]);

impl RelayId {
    pub fn random() -> Self {
        let mut bytes = [0; 32];
        getrandom::getrandom(&mut bytes).expect("no system random source");
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn keys(&self) -> Keys {
        Keys::derive(&self.0)
    }

    fn checksum(bytes: &[u8; 32]) -> [u8; RELAY_CHECKSUM_BYTES] {
        let digest = Sha256::new()
            .chain_update(RELAY_PREFIX)
            .chain_update(bytes)
            .finalize();
        let mut sum = [0; RELAY_CHECKSUM_BYTES];
        sum.copy_from_slice(&digest[..RELAY_CHECKSUM_BYTES]);
        sum
    }
}

impl fmt::Debug for RelayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RelayId(..)")
    }
}

impl fmt::Display for RelayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bytes = self.0.to_vec();
        bytes.extend_from_slice(&Self::checksum(&self.0));
        let text = encode(&bytes).to_ascii_lowercase();
        f.write_str(RELAY_PREFIX)?;
        for group in text.as_bytes().chunks(RELAY_GROUP) {
            f.write_str("-")?;
            f.write_str(std::str::from_utf8(group).map_err(|_| fmt::Error)?)?;
        }
        Ok(())
    }
}

impl FromStr for RelayId {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let rest = input
            .get(..RELAY_PREFIX.len())
            .filter(|p| p.eq_ignore_ascii_case(RELAY_PREFIX))
            .map(|_| &input[RELAY_PREFIX.len()..])
            .ok_or(IdError::Prefix)?;
        let bytes = decode(&symbols(rest, RELAY_CHARS)?);
        let mut id = [0; 32];
        id.copy_from_slice(&bytes[..32]);
        if bytes[32..] != Self::checksum(&id) {
            return Err(IdError::Checksum);
        }
        Ok(Self(id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OnlineId(u64);

impl OnlineId {
    pub const MAX: u64 = (1 << ONLINE_BITS) - 1;

    pub fn new(value: u64) -> Option<Self> {
        (value <= Self::MAX).then_some(Self(value))
    }

    pub fn random() -> Self {
        let mut bytes = [0; 8];
        getrandom::getrandom(&mut bytes[3..]).expect("no system random source");
        Self(u64::from_be_bytes(bytes))
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for OnlineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = encode(&self.0.to_be_bytes()[3..]);
        write!(f, "{}-{}", &text[..4], &text[4..])
    }
}

impl FromStr for OnlineId {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bytes = decode(&symbols(input, ONLINE_CHARS)?);
        let mut value = [0; 8];
        value[3..].copy_from_slice(&bytes);
        Ok(Self(u64::from_be_bytes(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RelayId {
        RelayId::from_bytes(std::array::from_fn(|i| (i * 37 + 5) as u8))
    }

    #[test]
    fn relay_id_format_and_parse_round_trip() {
        for _ in 0..64 {
            let id = RelayId::random();
            let text = id.to_string();
            assert_eq!(text.parse::<RelayId>(), Ok(id));
        }
    }

    #[test]
    fn relay_id_shape() {
        let text = sample().to_string();
        assert!(text.starts_with("cgb1-"));
        let groups: Vec<&str> = text["cgb1-".len()..].split('-').collect();
        assert_eq!(groups.len(), 7);
        assert!(groups.iter().all(|g| g.len() == 8));
        assert!(text.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn relay_id_parse_is_forgiving_about_case_spaces_and_lookalikes() {
        let id = sample();
        let text = id.to_string();
        assert_eq!(text.to_uppercase().parse::<RelayId>(), Ok(id.clone()));
        let spaced = format!("  {}  ", text.replace('-', " "));
        assert_eq!(spaced.parse::<RelayId>(), Ok(id.clone()));
        let joined = format!("cgb1{}", text["cgb1-".len()..].replace('-', ""));
        assert_eq!(joined.parse::<RelayId>(), Ok(id.clone()));
        let lookalikes = text["cgb1-".len()..].replace('0', "o").replace('1', "l");
        assert_eq!(format!("cgb1-{lookalikes}").parse::<RelayId>(), Ok(id));
    }

    #[test]
    fn relay_id_typo_is_caught() {
        let text = sample().to_string();
        let mut chars: Vec<char> = text.chars().collect();
        for at in "cgb1-".len()..chars.len() {
            if chars[at] == '-' {
                continue;
            }
            let original = chars[at];
            chars[at] = if original == 'x' { 'y' } else { 'x' };
            let typo: String = chars.iter().collect();
            assert_eq!(typo.parse::<RelayId>(), Err(IdError::Checksum), "{typo}");
            chars[at] = original;
        }
    }

    #[test]
    fn relay_id_swapped_neighbours_are_caught() {
        let text = sample().to_string();
        let mut chars: Vec<char> = text.chars().collect();
        let at = chars.len() - 3;
        chars.swap(at, at + 1);
        let swapped: String = chars.iter().collect();
        assert_ne!(swapped, text);
        assert_eq!(swapped.parse::<RelayId>(), Err(IdError::Checksum));
    }

    #[test]
    fn relay_id_parse_errors() {
        let text = sample().to_string();
        assert_eq!("".parse::<RelayId>(), Err(IdError::Prefix));
        assert_eq!(
            text.replace("cgb1", "cgb2").parse::<RelayId>(),
            Err(IdError::Prefix)
        );
        assert_eq!(
            text[..text.len() - 1].parse::<RelayId>(),
            Err(IdError::Length {
                found: 55,
                expected: 56
            })
        );
        assert_eq!(
            format!("{text}a").parse::<RelayId>(),
            Err(IdError::Length {
                found: 57,
                expected: 56
            })
        );
        let mut bad = text.clone();
        bad.replace_range(10..11, "u");
        assert_eq!(bad.parse::<RelayId>(), Err(IdError::Char('u')));
        bad.replace_range(10..11, "!");
        assert_eq!(bad.parse::<RelayId>(), Err(IdError::Char('!')));
        assert_eq!("cgb1-ü".parse::<RelayId>(), Err(IdError::Char('ü')));
    }

    #[test]
    fn relay_id_debug_hides_the_secret() {
        assert_eq!(format!("{:?}", sample()), "RelayId(..)");
    }

    #[test]
    fn online_id_format_and_parse() {
        let id = OnlineId::new(0x00_12_34_56_78).unwrap();
        let text = id.to_string();
        assert_eq!(text.len(), 9);
        assert_eq!(&text[4..5], "-");
        assert_eq!(text.parse::<OnlineId>(), Ok(id));
        assert_eq!(text.to_lowercase().replace('-', "").parse(), Ok(id));
        assert_eq!(OnlineId::new(0).unwrap().to_string(), "0000-0000");
        assert_eq!(
            OnlineId::new(OnlineId::MAX).unwrap().to_string(),
            "ZZZZ-ZZZZ"
        );
        assert_eq!(OnlineId::new(OnlineId::MAX + 1), None);
        for _ in 0..64 {
            let id = OnlineId::random();
            assert!(id.value() <= OnlineId::MAX);
            assert_eq!(id.to_string().parse::<OnlineId>(), Ok(id));
        }
    }

    #[test]
    fn online_id_has_no_confusable_letters() {
        for _ in 0..64 {
            let text = OnlineId::random().to_string();
            assert!(!text.contains(['O', 'I', 'L', 'U']), "{text}");
        }
    }

    #[test]
    fn online_id_parse_errors() {
        assert_eq!(
            "ABC-DEF".parse::<OnlineId>(),
            Err(IdError::Length {
                found: 6,
                expected: 8
            })
        );
        assert_eq!("ABCD-EFGU".parse::<OnlineId>(), Err(IdError::Char('U')));
        assert_eq!("0O1I-L000".parse::<OnlineId>(), "0011-1000".parse());
    }
}
