use base64::Engine;
use base64::engine::general_purpose::URL_SAFE;
use sha1::{Digest, Sha1};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Generation {
    Four,
    Five,
}

impl Generation {
    pub fn of_path(path: &str) -> Option<(Self, &str)> {
        if let Some(rest) = path.strip_prefix("/pokemondpds/") {
            Some((Self::Four, rest))
        } else {
            path.strip_prefix("/syachi2ds/web/")
                .map(|rest| (Self::Five, rest))
        }
    }

    pub fn number(self) -> i64 {
        match self {
            Self::Four => 4,
            Self::Five => 5,
        }
    }

    pub fn from_number(number: i64) -> Option<Self> {
        match number {
            4 => Some(Self::Four),
            5 => Some(Self::Five),
            _ => None,
        }
    }

    fn salt(self) -> &'static str {
        match self {
            Self::Four => "sAdeqWo3voLeC5r16DYv",
            Self::Five => "HZEdGCzcGGLvguqUEKQN",
        }
    }

    fn mask(self) -> u32 {
        match self {
            Self::Four => 0x4A3B_2C1D,
            Self::Five => 0x2DB8_42B2,
        }
    }

    fn stream(self) -> (u32, u32, u32) {
        match self {
            Self::Four => (0x45, 0x1111, 0x8000_0000),
            Self::Five => (0x1_D935, 0x2DD5, 8),
        }
    }

    fn header(self) -> usize {
        match self {
            Self::Four => 4,
            Self::Five => 8,
        }
    }

    pub fn record_bytes(self) -> usize {
        match self {
            Self::Four => 292,
            Self::Five => 296,
        }
    }
}

pub fn hash(generation: Generation, token: &str) -> String {
    hex(&Sha1::digest(
        format!("{}{token}", generation.salt()).as_bytes(),
    ))
}

pub fn sign(generation: Generation, mut body: Vec<u8>) -> Vec<u8> {
    if generation == Generation::Five {
        let salt = generation.salt();
        let text = format!("{salt}{}{salt}", URL_SAFE.encode(&body));
        body.extend_from_slice(hex(&Sha1::digest(text.as_bytes())).as_bytes());
    }
    body
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn keystream(generation: Generation, checksum: u32, bytes: &mut [u8]) {
    let (multiplier, increment, modulus) = generation.stream();
    let mut state = (checksum & 0xFFFF) | (checksum << 16);
    for byte in bytes {
        state = state.wrapping_mul(multiplier).wrapping_add(increment) % modulus;
        *byte ^= (state >> 16) as u8;
    }
}

fn byte_sum(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0u32, |sum, &byte| sum.wrapping_add(u32::from(byte)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Data {
    pub pid: i32,
    pub payload: Vec<u8>,
}

pub fn open(generation: Generation, text: &str) -> Option<Data> {
    let decoded = URL_SAFE.decode(text.as_bytes()).ok()?;
    let header: [u8; 4] = decoded.get(..4)?.try_into().ok()?;
    let checksum = u32::from_be_bytes(header) ^ generation.mask();
    let mut plain = decoded[4..].to_vec();
    keystream(generation, checksum, &mut plain);
    if byte_sum(&plain) != checksum || plain.len() < generation.header() {
        return None;
    }
    let pid = i32::from_le_bytes(plain[..4].try_into().ok()?);
    if generation == Generation::Five {
        let length = u32::from_le_bytes(plain[4..8].try_into().ok()?);
        if usize::try_from(length).ok()? != plain.len() - 8 {
            return None;
        }
    }
    Some(Data {
        pid,
        payload: plain[generation.header()..].to_vec(),
    })
}

#[cfg(test)]
pub fn seal(generation: Generation, pid: i32, payload: &[u8]) -> String {
    let mut plain = pid.to_le_bytes().to_vec();
    if generation == Generation::Five {
        plain.extend_from_slice(
            &u32::try_from(payload.len())
                .unwrap_or_default()
                .to_le_bytes(),
        );
    }
    plain.extend_from_slice(payload);
    let checksum = byte_sum(&plain);
    keystream(generation, checksum, &mut plain);
    let mut bytes = (checksum ^ generation.mask()).to_be_bytes().to_vec();
    bytes.extend_from_slice(&plain);
    URL_SAFE.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_data_opens_with_its_pid_in_both_generations() {
        for generation in [Generation::Four, Generation::Five] {
            let payload: Vec<u8> = (0..=255).collect();
            let text = seal(generation, 123_456, &payload);
            assert_eq!(
                open(generation, &text),
                Some(Data {
                    pid: 123_456,
                    payload: payload.clone()
                })
            );
        }
    }

    #[test]
    fn gen_four_data_is_scrambled_and_a_changed_byte_fails_the_sum() {
        let text = seal(Generation::Four, 7, &[0; 32]);
        let bytes = URL_SAFE.decode(&text).unwrap();
        assert!(bytes[8..].iter().any(|&byte| byte != 0));
        let mut broken = bytes.clone();
        broken[10] ^= 1;
        assert_eq!(open(Generation::Four, &URL_SAFE.encode(broken)), None);
        assert_eq!(open(Generation::Four, "%%%"), None);
    }

    #[test]
    fn gen_five_data_travels_as_plain_bytes() {
        let text = seal(Generation::Five, 9, &[1, 2, 3]);
        let bytes = URL_SAFE.decode(&text).unwrap();
        assert_eq!(&bytes[4..], &[9, 0, 0, 0, 3, 0, 0, 0, 1, 2, 3]);
    }

    #[test]
    fn the_hash_is_the_salted_token_in_lowercase_hex() {
        let hash = hash(Generation::Four, "abc");
        assert_eq!(hash.len(), 40);
        assert_eq!(hash, hex(&Sha1::digest(b"sAdeqWo3voLeC5r16DYvabc")));
    }

    #[test]
    fn only_gen_five_answers_carry_a_signature() {
        assert_eq!(sign(Generation::Four, vec![1, 0]), vec![1, 0]);
        let signed = sign(Generation::Five, vec![1, 0]);
        assert_eq!(signed.len(), 42);
        assert!(signed[2..].iter().all(u8::is_ascii_hexdigit));
    }

    #[test]
    fn paths_pick_the_generation() {
        assert_eq!(
            Generation::of_path("/pokemondpds/worldexchange/info.asp"),
            Some((Generation::Four, "worldexchange/info.asp"))
        );
        assert_eq!(
            Generation::of_path("/syachi2ds/web/common/setProfile.asp"),
            Some((Generation::Five, "common/setProfile.asp"))
        );
        assert_eq!(Generation::of_path("/other"), None);
    }
}
