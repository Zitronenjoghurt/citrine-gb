use crate::crypto::PublicKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

pub const SAFETY_CODE_GROUPS: usize = 8;
const GROUPS_PER_KEY: usize = SAFETY_CODE_GROUPS / 2;
const ROUNDS: u32 = 8192;
const LABEL: &[u8] = b"citrine friend code 1";
const CHECK_LABEL: &[u8] = b"citrine screen check 1";
const COMMIT_LABEL: &[u8] = b"citrine screen commit 1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SafetyCode([u32; SAFETY_CODE_GROUPS]);

impl SafetyCode {
    pub fn new(a: &PublicKey, b: &PublicKey) -> Self {
        let (first, second) = if a.0 <= b.0 { (a, b) } else { (b, a) };
        let mut groups = [0; SAFETY_CODE_GROUPS];
        groups[..GROUPS_PER_KEY].copy_from_slice(&half(first));
        groups[GROUPS_PER_KEY..].copy_from_slice(&half(second));
        Self(groups)
    }

    pub fn lines(&self) -> [String; 2] {
        let line = |groups: &[u32]| {
            groups
                .iter()
                .map(|&g| group_text(g))
                .collect::<Vec<_>>()
                .join(" ")
        };
        [
            line(&self.0[..GROUPS_PER_KEY]),
            line(&self.0[GROUPS_PER_KEY..]),
        ]
    }
}

impl fmt::Display for SafetyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [first, second] = self.lines();
        write!(f, "{first} {second}")
    }
}

fn half(key: &PublicKey) -> [u32; GROUPS_PER_KEY] {
    let mut digest: [u8; 32] = Sha256::new()
        .chain_update(LABEL)
        .chain_update(key.0)
        .finalize()
        .into();
    for _ in 0..ROUNDS {
        digest = Sha256::new()
            .chain_update(digest)
            .chain_update(key.0)
            .finalize()
            .into();
    }
    groups(&digest)
}

fn groups<const N: usize>(digest: &[u8; 32]) -> [u32; N] {
    let mut groups = [0; N];
    for (group, bytes) in groups.iter_mut().zip(digest.chunks(5)) {
        let value = bytes.iter().fold(0u64, |v, &b| (v << 8) | b as u64);
        *group = (value % 100_000) as u32;
    }
    groups
}

fn group_text(group: u32) -> String {
    format!("{group:05}")
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckNonce([u8; 32]);

impl CheckNonce {
    pub fn random() -> Self {
        let mut bytes = [0; 32];
        getrandom::getrandom(&mut bytes).expect("no system random source");
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn commit(&self) -> CheckCommit {
        let digest: [u8; 32] = Sha256::new()
            .chain_update(COMMIT_LABEL)
            .chain_update(self.0)
            .finalize()
            .into();
        CheckCommit(digest)
    }
}

impl fmt::Debug for CheckNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CheckNonce(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckCommit(pub [u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckCode(u32);

impl CheckCode {
    pub fn new(binding: &[u8; 32], nonce: &CheckNonce) -> Self {
        let digest: [u8; 32] = Sha256::new()
            .chain_update(CHECK_LABEL)
            .chain_update(binding)
            .chain_update(nonce.0)
            .finalize()
            .into();
        let [group] = groups(&digest);
        Self(group)
    }

    pub fn lines(&self) -> [String; 1] {
        [group_text(self.0)]
    }
}

impl fmt::Display for CheckCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&group_text(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_friends_see_the_same_code() {
        let ana = PublicKey([1; 32]);
        let ben = PublicKey([2; 32]);
        assert_eq!(SafetyCode::new(&ana, &ben), SafetyCode::new(&ben, &ana));
        assert_eq!(
            SafetyCode::new(&ana, &ben).to_string(),
            "88194 38371 57551 00612 24650 06303 55842 86865"
        );
    }

    #[test]
    fn a_different_key_on_either_side_changes_the_code() {
        let ana = PublicKey([1; 32]);
        let ben = PublicKey([2; 32]);
        let mut other = [2; 32];
        other[31] = 3;
        let relay = PublicKey(other);
        let code = SafetyCode::new(&ana, &ben);
        assert_ne!(SafetyCode::new(&ana, &relay), code);
        assert_ne!(SafetyCode::new(&relay, &ben), code);
    }

    #[test]
    fn a_check_code_is_one_group_of_the_friend_code_digits() {
        let code = CheckCode::new(&[7; 32], &CheckNonce::from_bytes([9; 32]));
        assert_eq!(code.to_string().len(), 5);
        assert!(code.to_string().bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(code.lines(), [code.to_string()]);
        assert_eq!(
            code,
            CheckCode::new(&[7; 32], &CheckNonce::from_bytes([9; 32]))
        );
    }

    #[test]
    fn another_binding_or_nonce_changes_the_check_code() {
        let nonce = CheckNonce::from_bytes([9; 32]);
        let code = CheckCode::new(&[7; 32], &nonce);
        assert_ne!(CheckCode::new(&[8; 32], &nonce), code);
        assert_ne!(
            CheckCode::new(&[7; 32], &CheckNonce::from_bytes([10; 32])),
            code
        );
    }

    #[test]
    fn a_commit_names_one_nonce_and_hides_it() {
        let nonce = CheckNonce::from_bytes([9; 32]);
        assert_eq!(nonce.commit(), CheckNonce::from_bytes([9; 32]).commit());
        assert_ne!(nonce.commit(), CheckNonce::from_bytes([10; 32]).commit());
        assert_ne!(nonce.commit().0, [9; 32]);
        assert_ne!(CheckNonce::random(), CheckNonce::random());
    }

    #[test]
    fn the_code_reads_as_two_lines_of_four_groups() {
        let lines = SafetyCode::new(&PublicKey([1; 32]), &PublicKey([2; 32])).lines();
        assert_eq!(lines[0], "88194 38371 57551 00612");
        assert_eq!(lines[1], "24650 06303 55842 86865");
    }
}
