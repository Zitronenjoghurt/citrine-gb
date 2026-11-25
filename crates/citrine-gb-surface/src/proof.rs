use crate::pairing::{Code, Tag};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

pub type Nonce = [u8; 16];
pub type LinkKey = [u8; 32];

const LINK_CONTEXT: &[u8] = b"citrine screen link";
const RESUME_CONTEXT: &[u8] = b"citrine screen resume";
const CODE_CONTEXT: &[u8] = b"citrine screen code";
const TAG_CONTEXT: &[u8] = b"citrine nearby tag";
const CODE_PHONE: &[u8] = b"citrine screen phone";
const CODE_HOST: &[u8] = b"citrine screen host";
pub const OPEN_LINK: LinkKey = [0; 32];

pub fn nonce() -> Nonce {
    let mut nonce = [0; 16];
    getrandom::getrandom(&mut nonce).expect("the system has randomness");
    nonce
}

fn mac(key: &[u8], context: &[u8], nonce: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(context);
    mac.update(nonce);
    mac
}

fn link_key(key: &[u8], context: &[u8]) -> LinkKey {
    mac(key, context, &[]).finalize().into_bytes().into()
}

pub fn resume_link(binding: &[u8; 32]) -> LinkKey {
    link_key(binding, RESUME_CONTEXT)
}

pub(crate) fn code_key(shared: &[u8]) -> LinkKey {
    link_key(shared, CODE_CONTEXT)
}

fn code_sides(code: Code) -> (Password, Identity, Identity) {
    (
        Password::new(code.plain()),
        Identity::new(CODE_PHONE),
        Identity::new(CODE_HOST),
    )
}

pub struct CodeKnock(Spake2<Ed25519Group>);

impl CodeKnock {
    pub fn start(code: Code) -> (Self, Vec<u8>) {
        let (password, phone, host) = code_sides(code);
        let (state, knock) = Spake2::<Ed25519Group>::start_a(&password, &phone, &host);
        (Self(state), knock)
    }

    pub fn finish(self, answer: &[u8]) -> Option<LinkKey> {
        self.0.finish(answer).ok().map(|shared| code_key(&shared))
    }
}

pub fn code_answer(code: Code, knock: &[u8]) -> Option<(Vec<u8>, LinkKey)> {
    let (password, phone, host) = code_sides(code);
    let (state, answer) = Spake2::<Ed25519Group>::start_b(&password, &phone, &host);
    let shared = state.finish(knock).ok()?;
    Some((answer, code_key(&shared)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProofKey(pub [u8; 32]);

impl ProofKey {
    pub fn link(&self) -> LinkKey {
        link_key(&self.0, LINK_CONTEXT)
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
    fn link_keys_differ_by_secret_and_use() {
        let own = ProofKey([4; 32]).link();
        assert_ne!(own, ProofKey([5; 32]).link());
        assert_ne!(own, [4; 32]);
        assert_ne!(own, OPEN_LINK);
        assert_ne!(resume_link(&[4; 32]), resume_link(&[5; 32]));
        assert_ne!(resume_link(&[4; 32]), [4; 32]);
        assert_ne!(resume_link(&[4; 32]), own);
    }

    #[test]
    fn a_code_knock_shares_a_key_only_with_the_same_code() {
        let code = Code::parse("123 456").unwrap();
        let (knock, first) = CodeKnock::start(code);
        let (answer, host_key) = code_answer(Code::parse("123456").unwrap(), &first).unwrap();
        assert_eq!(knock.finish(&answer), Some(host_key));

        let (knock, first) = CodeKnock::start(code);
        let (answer, host_key) = code_answer(Code::parse("123 457").unwrap(), &first).unwrap();
        assert_ne!(knock.finish(&answer), Some(host_key));

        let (one, _) = CodeKnock::start(code);
        let (two, first) = CodeKnock::start(code);
        let (answer, _) = code_answer(code, &first).unwrap();
        assert_ne!(one.finish(&answer), two.finish(&answer));
        assert_eq!(code_answer(code, &[1; 7]), None);
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
