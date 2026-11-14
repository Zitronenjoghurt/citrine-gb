use crate::pairing::Tag;
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub type Nonce = [u8; 16];
pub type Proof = [u8; 32];

const PROOF_CONTEXT: &[u8] = b"citrine screen proof";
const TAG_CONTEXT: &[u8] = b"citrine nearby tag";

pub fn nonce() -> Nonce {
    let mut nonce = [0; 16];
    getrandom::getrandom(&mut nonce).expect("the system has randomness");
    nonce
}

pub fn instance() -> u64 {
    let mut bytes = [0; 8];
    getrandom::getrandom(&mut bytes).expect("the system has randomness");
    u64::from_le_bytes(bytes)
}

fn mac(key: &[u8; 32], context: &[u8], nonce: &Nonce) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(context);
    mac.update(nonce);
    mac
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProofKey(pub [u8; 32]);

impl ProofKey {
    pub fn prove(&self, nonce: &Nonce, host: u64) -> Proof {
        let mut mac = mac(&self.0, PROOF_CONTEXT, nonce);
        mac.update(&host.to_le_bytes());
        mac.finalize().into_bytes().into()
    }

    pub fn checks(&self, nonce: &Nonce, host: u64, proof: &Proof) -> bool {
        let mut mac = mac(&self.0, PROOF_CONTEXT, nonce);
        mac.update(&host.to_le_bytes());
        mac.verify_slice(proof).is_ok()
    }
}

impl std::fmt::Debug for ProofKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProofKey(..)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TagKey(pub [u8; 32]);

impl TagKey {
    pub fn tag(&self, nonce: &Nonce) -> Tag {
        let full = mac(&self.0, TAG_CONTEXT, nonce).finalize().into_bytes();
        let mut tag = [0; 8];
        tag.copy_from_slice(&full[..8]);
        tag
    }

    pub fn tags(&self, nonce: &Nonce, tag: &Tag) -> bool {
        mac(&self.0, TAG_CONTEXT, nonce)
            .verify_truncated_left(tag)
            .is_ok()
    }
}

impl std::fmt::Debug for TagKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TagKey(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proof_holds_for_its_nonce_and_host_only() {
        let key = ProofKey([4; 32]);
        let nonce = nonce();
        let proof = key.prove(&nonce, 7);
        assert!(key.checks(&nonce, 7, &proof));
        assert!(!key.checks(&nonce, 8, &proof));
        assert!(!key.checks(&super::nonce(), 7, &proof));
        assert!(!ProofKey([5; 32]).checks(&nonce, 7, &proof));
        let mut bent = proof;
        bent[31] ^= 1;
        assert!(!key.checks(&nonce, 7, &bent));
    }

    #[test]
    fn a_tag_changes_with_the_nonce_and_matches_its_key() {
        let key = TagKey([6; 32]);
        let (one, two) = (nonce(), nonce());
        assert_ne!(one, two);
        assert_ne!(key.tag(&one), key.tag(&two));
        assert!(key.tags(&one, &key.tag(&one)));
        assert!(!key.tags(&two, &key.tag(&one)));
        assert!(!TagKey([7; 32]).tags(&one, &key.tag(&one)));
    }
}
