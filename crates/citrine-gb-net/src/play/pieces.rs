use super::PlayMsg;
use super::wire::Wire;
use crate::chat::{pack, unpack};
use std::borrow::Cow;

pub const WHOLE_BYTES: usize = 768 * 1024;
pub const PIECE_BYTES: usize = 192 * 1024;
pub const MAX_PIECED_BYTES: usize = 24 << 20;

impl PlayMsg {
    pub(super) fn bodies(&self, last_id: &mut u32) -> Vec<Vec<u8>> {
        let bytes = postcard::to_allocvec(self).expect("session messages always encode");
        if bytes.len() <= WHOLE_BYTES {
            return vec![bytes];
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
                postcard::to_allocvec(&piece).expect("pieces always encode")
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
pub(super) struct Pieces {
    joining: Option<Joining>,
}

impl Pieces {
    pub(super) fn read(&mut self, wire: Wire) -> Option<PlayMsg> {
        match wire {
            Wire::Piece {
                id,
                offset,
                total,
                data,
            } => self.join(id, offset, total, &data),
            wire => wire.into_msg(),
        }
    }

    pub(super) fn receiving(&self) -> Option<(u32, u32)> {
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
    use crate::crypto::{Keys, OpenError};
    use crate::{ClientMsg, CodecError, Origin, RejoinPoint, RelayId};

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
            budget: 0,
        }
    }

    fn read(pieces: &mut Pieces, body: &[u8]) -> Option<PlayMsg> {
        let (wire, rest) = postcard::take_from_bytes::<Wire>(body).unwrap();
        assert!(rest.is_empty());
        pieces.read(wire)
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
    fn a_resync_crosses_packed() {
        let bodies = heartgold_resync().bodies(&mut 0);
        let bytes: usize = bodies.iter().map(Vec::len).sum();
        assert!(bytes < 5 << 19, "{bytes} bytes");
    }

    #[test]
    fn small_messages_stay_whole() {
        let msg = PlayMsg::Rejoin {
            round: 1,
            from: RejoinPoint {
                session: 9,
                round: 1,
                frame: 300,
                oldest: 280,
            },
        };
        let bodies = msg.bodies(&mut 0);
        assert_eq!(bodies.len(), 1);
        assert_eq!(read(&mut Pieces::default(), &bodies[0]), Some(msg));
    }

    #[test]
    fn a_missing_piece_drops_the_message_and_the_next_one_arrives() {
        let mut last = 0;
        let mut pieces = Pieces::default();
        let first = heartgold_resume().bodies(&mut last);
        for (i, body) in first.iter().enumerate() {
            if i != 2 {
                assert_eq!(read(&mut pieces, body), None);
            }
        }
        let msg = heartgold_resync();
        let out: Vec<PlayMsg> = msg
            .bodies(&mut last)
            .iter()
            .filter_map(|body| read(&mut pieces, body))
            .collect();
        assert_eq!(out, vec![msg]);
    }

    #[test]
    fn a_piece_claiming_more_than_the_limit_is_dropped() {
        let piece = Wire::Piece {
            id: 1,
            offset: 0,
            total: MAX_PIECED_BYTES as u32 + 1,
            data: Cow::Owned(vec![0; 16]),
        };
        let mut pieces = Pieces::default();
        assert_eq!(pieces.read(piece), None);
        assert_eq!(pieces.receiving(), None);
    }

    #[test]
    fn a_lone_piece_never_opens_as_a_whole_message() {
        let (a, b) = pair();
        let bodies = heartgold_resume().bodies(&mut 0);
        let sealed = a.seal(&b.public_key(), &bodies[0]);
        assert_eq!(PlayMsg::open(&b, &a.public_key(), &sealed), Err(OpenError));
    }
}
