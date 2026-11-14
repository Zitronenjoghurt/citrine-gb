use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(crate) struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub(crate) fn bytes(&mut self, data: &[u8]) {
        for byte in data {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    pub(crate) fn samples(&mut self, samples: &[f32]) {
        for sample in samples {
            self.bytes(&sample.to_bits().to_le_bytes());
        }
    }

    pub(crate) fn finish(self) -> String {
        format!("{:016x}", self.0)
    }
}

pub(crate) fn hash(data: &[u8]) -> String {
    let mut fnv = Fnv::default();
    fnv.bytes(data);
    fnv.finish()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Digest {
    pub(crate) frames: Vec<String>,
    pub(crate) audio: String,
    pub(crate) state: String,
    #[serde(default)]
    pub(crate) facts: BTreeMap<String, u64>,
}

impl Digest {
    pub(crate) fn differences(&self, expected: &Digest) -> Vec<String> {
        let mut out = Vec::new();
        if self.frames.len() != expected.frames.len() {
            out.push(format!(
                "{} frame checkpoints, expected {}",
                self.frames.len(),
                expected.frames.len()
            ));
        }
        for (index, (got, want)) in self.frames.iter().zip(&expected.frames).enumerate() {
            if got != want {
                out.push(format!(
                    "frame checkpoint {} is {got}, expected {want}",
                    index + 1
                ));
            }
        }
        if self.audio != expected.audio {
            out.push(format!(
                "audio is {}, expected {}",
                self.audio, expected.audio
            ));
        }
        if self.state != expected.state {
            out.push(format!(
                "state is {}, expected {}",
                self.state, expected.state
            ));
        }
        let names = self.facts.keys().chain(expected.facts.keys());
        let mut seen = std::collections::BTreeSet::new();
        for name in names {
            if !seen.insert(name) {
                continue;
            }
            let got = self.facts.get(name);
            let want = expected.facts.get(name);
            if got != want {
                out.push(format!("{name} is {got:?}, expected {want:?}"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_the_published_vector() {
        assert_eq!(hash(b"a"), "af63dc4c8601ec8c");
    }

    #[test]
    fn a_changed_fact_is_a_difference() {
        let mut facts = BTreeMap::new();
        facts.insert("state_bytes".to_string(), 10);
        let expected = Digest {
            frames: vec!["1".into()],
            facts: facts.clone(),
            ..Digest::default()
        };
        facts.insert("state_bytes".to_string(), 11);
        let got = Digest {
            facts,
            ..expected.clone()
        };
        assert_eq!(got.differences(&expected).len(), 1);
        assert!(expected.differences(&expected).is_empty());
    }
}
