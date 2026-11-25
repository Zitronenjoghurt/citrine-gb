use serde::de::{Error, SeqAccess, Visitor};
use serde::{Deserializer, Serializer};
use std::fmt;

pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(bytes)
}

const HINT_BYTES: usize = 1 << 20;

pub(crate) fn cautious<T>(hint: Option<usize>) -> usize {
    hint.unwrap_or(0).min(HINT_BYTES / size_of::<T>().max(1))
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Box<[u8]>, D::Error> {
    deserializer.deserialize_byte_buf(Bytes)
}

struct Bytes;

impl<'de> Visitor<'de> for Bytes {
    type Value = Box<[u8]>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bytes")
    }

    fn visit_bytes<E: Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
        Ok(bytes.into())
    }

    fn visit_byte_buf<E: Error>(self, bytes: Vec<u8>) -> Result<Self::Value, E> {
        Ok(bytes.into_boxed_slice())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut bytes = Vec::with_capacity(cautious::<u8>(seq.size_hint()));
        while let Some(byte) = seq.next_element()? {
            bytes.push(byte);
        }
        Ok(bytes.into_boxed_slice())
    }
}

pub mod shared {
    use serde::{Deserializer, Serializer};
    use std::sync::Arc;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        super::serialize(bytes, serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<[u8]>, D::Error> {
        super::deserialize(deserializer).map(Arc::from)
    }
}

pub mod array {
    use serde::de::Error;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer, const N: usize>(
        bytes: &[u8; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        deserializer: D,
    ) -> Result<[u8; N], D::Error> {
        let bytes = super::deserialize(deserializer)?;
        <[u8; N]>::try_from(&*bytes).map_err(|_| D::Error::invalid_length(bytes.len(), &"bytes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_claim_reserves_at_most_a_mebibyte() {
        assert_eq!(cautious::<u8>(Some(usize::MAX)), HINT_BYTES);
        assert_eq!(cautious::<u64>(Some(usize::MAX)), HINT_BYTES / 8);
        assert_eq!(cautious::<u8>(Some(12)), 12);
        assert_eq!(cautious::<()>(None), 0);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn a_byte_list_that_claims_four_gib_is_refused() {
        let claim = [0xDD, 0xFF, 0xFF, 0xFF, 0xFF, 0x01];
        let mut from = rmp_serde::Deserializer::from_read_ref(&claim[..]);
        assert!(deserialize(&mut from).is_err());
    }
}
