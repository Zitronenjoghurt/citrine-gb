use citrine_gb_net::{OnlineId, PublicKey};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "BTreeMap<String, String>", into = "BTreeMap<String, String>")]
pub struct FriendPins(BTreeMap<OnlineId, PublicKey>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pin {
    First,
    Same,
    Changed(PublicKey),
}

impl FriendPins {
    pub fn check(&mut self, id: OnlineId, key: PublicKey) -> Pin {
        match self.0.get(&id) {
            None => {
                self.0.insert(id, key);
                Pin::First
            }
            Some(pinned) if *pinned == key => Pin::Same,
            Some(pinned) => Pin::Changed(*pinned),
        }
    }

    pub fn trust(&mut self, id: OnlineId, key: PublicKey) {
        self.0.insert(id, key);
    }

    pub fn get(&self, id: OnlineId) -> Option<PublicKey> {
        self.0.get(&id).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<BTreeMap<String, String>> for FriendPins {
    fn from(saved: BTreeMap<String, String>) -> Self {
        Self(
            saved
                .iter()
                .filter_map(|(id, key)| Some((id.parse().ok()?, from_hex(key)?)))
                .collect(),
        )
    }
}

impl From<FriendPins> for BTreeMap<String, String> {
    fn from(pins: FriendPins) -> Self {
        pins.0
            .iter()
            .map(|(id, key)| (id.to_string(), to_hex(key)))
            .collect()
    }
}

fn to_hex(key: &PublicKey) -> String {
    key.0.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> Option<PublicKey> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut key = [0; 32];
    for (byte, pair) in key.iter_mut().zip(text.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(PublicKey(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> OnlineId {
        OnlineId::new(n).unwrap()
    }

    #[test]
    fn the_first_key_is_kept_and_a_different_one_is_refused() {
        let mut pins = FriendPins::default();
        assert_eq!(pins.check(id(1), PublicKey([1; 32])), Pin::First);
        assert_eq!(pins.check(id(1), PublicKey([1; 32])), Pin::Same);
        assert_eq!(
            pins.check(id(1), PublicKey([9; 32])),
            Pin::Changed(PublicKey([1; 32]))
        );
        assert_eq!(pins.get(id(1)), Some(PublicKey([1; 32])));
        assert_eq!(pins.check(id(2), PublicKey([9; 32])), Pin::First);
    }

    #[test]
    fn trusting_a_new_key_replaces_the_pin() {
        let mut pins = FriendPins::default();
        pins.check(id(1), PublicKey([1; 32]));
        pins.trust(id(1), PublicKey([9; 32]));
        assert_eq!(pins.check(id(1), PublicKey([9; 32])), Pin::Same);
    }

    #[test]
    fn pins_survive_the_settings_file_and_bad_entries_are_dropped() {
        let mut pins = FriendPins::default();
        pins.check(id(1), PublicKey([0xab; 32]));
        let text = serde_json::to_string(&pins).unwrap();
        assert_eq!(text, format!("{{\"{}\":\"{}\"}}", id(1), "ab".repeat(32)));
        let back: FriendPins = serde_json::from_str(&text).unwrap();
        assert_eq!(back, pins);
        let bad = format!("{{\"{}\":\"zz\",\"nope\":\"{}\"}}", id(2), "ab".repeat(32));
        let loaded: FriendPins = serde_json::from_str(&bad).unwrap();
        assert!(loaded.is_empty());
    }
}
