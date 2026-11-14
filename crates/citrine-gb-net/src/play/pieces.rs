use super::PlayMsg;
use super::wire::Wire;
use crate::chat::{pack, unpack};
use crate::crypto::{Keys, OpenError, PublicKey, Sealed};
use std::borrow::Cow;

pub const WHOLE_BYTES: usize = 768 * 1024;
pub const PIECE_BYTES: usize = 192 * 1024;
pub const MAX_PIECED_BYTES: usize = 24 << 20;

impl PlayMsg {
    pub fn seal_in_pieces(&self, keys: &Keys, to: &PublicKey, last_id: &mut u32) -> Vec<Sealed> {
        let bytes = postcard::to_allocvec(self).expect("session messages always encode");
        if bytes.len() <= WHOLE_BYTES {
            return vec![keys.seal(to, &bytes)];
        }
        let packed = pack(&bytes, true);
        if packed.len() > MAX_PIECED_BYTES {
            return Vec::new();
        }
        *last_id = last_id.wrapping_add(1);
        let total = packed.len() as u32;
        packed
            .chunks(PIECE_BYTES)
            .enumerate()
            .map(|(i, data)| {
                let piece = Wire::Piece {
                    id: *last_id,
                    offset: (i * PIECE_BYTES) as u32,
                    total,
                    data: Cow::Borrowed(data),
                };
                super::seal(&piece, keys, to)
            })
            .collect()
    }
}

struct Joining {
    id: u32,
    total: u32,
    data: Vec<u8>,
}

#[derive(Default)]
pub struct Pieces {
    joining: Option<Joining>,
}

impl Pieces {
    pub fn open(
        &mut self,
        keys: &Keys,
        from: &PublicKey,
        sealed: &Sealed,
    ) -> Result<Option<PlayMsg>, OpenError> {
        let bytes = keys.open(from, sealed)?;
        let wire: Wire = match postcard::take_from_bytes(&bytes) {
            Ok((wire, [])) => wire,
            _ => return Err(OpenError),
        };
        Ok(match wire {
            Wire::Piece {
                id,
                offset,
                total,
                data,
            } => self.join(id, offset, total, &data),
            wire => wire.into_msg(),
        })
    }

    pub fn receiving(&self) -> Option<(u32, u32)> {
        self.joining
            .as_ref()
            .map(|j| (j.data.len() as u32, j.total))
    }

    fn join(&mut self, id: u32, offset: u32, total: u32, data: &[u8]) -> Option<PlayMsg> {
        if offset == 0 {
            self.joining = (total as usize <= MAX_PIECED_BYTES).then(|| Joining {
                id,
                total,
                data: Vec::new(),
            });
        }
        let joining = self.joining.as_mut()?;
        if (joining.id, joining.total) != (id, total)
            || joining.data.len() != offset as usize
            || joining.data.len() + data.len() > total as usize
        {
            self.joining = None;
            return None;
        }
        joining.data.extend_from_slice(data);
        if joining.data.len() < total as usize {
            return None;
        }
        let joined = self.joining.take()?;
        let bytes = unpack(&joined.data, MAX_PIECED_BYTES).ok()?;
        match postcard::take_from_bytes::<Wire>(&bytes) {
            Ok((wire, [])) => wire.into_msg(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMsg, CodecError, MAX_FRAME_BYTES, Origin, RejoinPoint, RelayId};

    fn pair() -> (Keys, Keys) {
        (RelayId::random().keys(), RelayId::random().keys())
    }

    fn noise(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    fn heartgold_resync() -> PlayMsg {
        let mut data = noise(2 << 20, 7);
        data.resize(7 << 20, 0);
        PlayMsg::Resync {
            round: 3,
            frame: 4_000,
            unit: 1,
            data,
            cable: None,
        }
    }

    fn heartgold_resume() -> PlayMsg {
        PlayMsg::Setup {
            round: 3,
            from: Origin::Resume(pack(&noise(2 << 20, 11), true)),
        }
    }

    fn frames(sealed: Vec<Sealed>) -> Vec<Sealed> {
        for sealed in &sealed {
            let msg = ClientMsg::Play {
                session: u64::MAX,
                sealed: sealed.clone(),
            };
            assert!(msg.encode().unwrap().len() <= MAX_FRAME_BYTES);
        }
        sealed
    }

    #[test]
    fn heartgold_sized_states_are_too_large_for_one_frame() {
        let (a, b) = pair();
        for msg in [heartgold_resync(), heartgold_resume()] {
            let sealed = msg.seal(&a, &b.public_key());
            let frame = ClientMsg::Play { session: 1, sealed };
            assert!(matches!(frame.encode(), Err(CodecError::TooLarge(_))));
        }
    }

    #[test]
    fn heartgold_sized_states_cross_in_pieces_that_fit_a_frame() {
        let (a, b) = pair();
        let mut last = 0;
        let mut pieces = Pieces::default();
        for msg in [heartgold_resync(), heartgold_resume()] {
            let sealed = frames(msg.seal_in_pieces(&a, &b.public_key(), &mut last));
            assert!(sealed.len() > 1);
            let mut seen = Vec::new();
            let mut out = Vec::new();
            for sealed in &sealed {
                out.extend(pieces.open(&b, &a.public_key(), sealed).unwrap());
                seen.extend(pieces.receiving());
            }
            assert_eq!(out, vec![msg]);
            assert_eq!(pieces.receiving(), None);
            assert!(seen.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 == w[1].1));
        }
    }

    #[test]
    fn a_resync_crosses_packed() {
        let (a, b) = pair();
        let sealed = heartgold_resync().seal_in_pieces(&a, &b.public_key(), &mut 0);
        let bytes: usize = sealed.iter().map(|s| s.ciphertext.len()).sum();
        assert!(bytes < 5 << 19, "{bytes} bytes");
    }

    #[test]
    fn small_messages_stay_whole_and_open_as_before() {
        let (a, b) = pair();
        let msg = PlayMsg::Rejoin {
            round: 1,
            from: RejoinPoint {
                session: 9,
                round: 1,
                frame: 300,
                oldest: 280,
            },
        };
        let sealed = msg.seal_in_pieces(&a, &b.public_key(), &mut 0);
        assert_eq!(sealed.len(), 1);
        assert_eq!(
            PlayMsg::open(&b, &a.public_key(), &sealed[0]),
            Ok(msg.clone())
        );
        let opened = Pieces::default().open(&b, &a.public_key(), &sealed[0]);
        assert_eq!(opened, Ok(Some(msg)));
    }

    #[test]
    fn a_missing_piece_drops_the_message_and_the_next_one_arrives() {
        let (a, b) = pair();
        let mut last = 0;
        let mut pieces = Pieces::default();
        let first = heartgold_resume().seal_in_pieces(&a, &b.public_key(), &mut last);
        for (i, sealed) in first.iter().enumerate() {
            if i != 2 {
                let out = pieces.open(&b, &a.public_key(), sealed).unwrap();
                assert_eq!(out, None);
            }
        }
        let msg = heartgold_resync();
        let mut out = Vec::new();
        for sealed in msg.seal_in_pieces(&a, &b.public_key(), &mut last) {
            out.extend(pieces.open(&b, &a.public_key(), &sealed).unwrap());
        }
        assert_eq!(out, vec![msg]);
    }

    #[test]
    fn a_piece_claiming_more_than_the_limit_is_dropped() {
        let (a, b) = pair();
        let piece = Wire::Piece {
            id: 1,
            offset: 0,
            total: MAX_PIECED_BYTES as u32 + 1,
            data: Cow::Owned(vec![0; 16]),
        };
        let sealed = crate::play::seal(&piece, &a, &b.public_key());
        let mut pieces = Pieces::default();
        assert_eq!(pieces.open(&b, &a.public_key(), &sealed), Ok(None));
        assert_eq!(pieces.receiving(), None);
    }

    #[test]
    fn a_lone_piece_never_opens_as_a_whole_message() {
        let (a, b) = pair();
        let sealed = heartgold_resume().seal_in_pieces(&a, &b.public_key(), &mut 0);
        assert_eq!(
            PlayMsg::open(&b, &a.public_key(), &sealed[0]),
            Err(OpenError)
        );
    }
}
