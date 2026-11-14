use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(_: &(), serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_none()
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
    IgnoredAny::deserialize(deserializer).map(|_| ())
}

#[cfg(all(test, feature = "persistence"))]
mod tests {
    #[derive(serde::Serialize)]
    struct Before {
        kept: u8,
        gone: Option<u16>,
        after: bool,
    }

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct After {
        kept: u8,
        #[serde(with = "crate::utils::retired")]
        gone: (),
        after: bool,
    }

    #[test]
    fn a_state_saved_while_the_field_held_a_value_still_loads() {
        let before = Before {
            kept: 3,
            gone: Some(0x1230),
            after: true,
        };
        let loaded: After = rmp_serde::from_slice(&rmp_serde::to_vec(&before).unwrap()).unwrap();
        assert_eq!(
            loaded,
            After {
                kept: 3,
                gone: (),
                after: true,
            }
        );
    }

    #[test]
    fn a_retired_field_saves_the_bytes_an_empty_one_did() {
        let before = Before {
            kept: 3,
            gone: None,
            after: true,
        };
        let after = After {
            kept: 3,
            gone: (),
            after: true,
        };
        assert_eq!(
            rmp_serde::to_vec(&after).unwrap(),
            rmp_serde::to_vec(&before).unwrap()
        );
    }
}
