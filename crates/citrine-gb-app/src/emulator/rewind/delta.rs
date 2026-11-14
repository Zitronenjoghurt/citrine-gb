use std::collections::HashMap;
use std::ops::Range;

const MIN_PART: usize = 16 * 1024;
const BLOCK: usize = 4096;
const ROOT: u64 = 0x243F_6A88_85A3_08D3;
const AFTER: u64 = 0x1319_8A2E_0370_7344;
const GAP: u64 = 0xA409_3822_299F_31D0;
const TAIL: u64 = 0x082E_FA98_EC4E_6C89;
const SAME: u8 = 0;
const RAW: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Part {
    key: u64,
    start: usize,
    len: usize,
}

impl Part {
    fn range(&self) -> Range<usize> {
        self.start..self.start + self.len
    }
}

fn mix(key: u64, index: u64) -> u64 {
    (key ^ index.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .rotate_left(27)
        .wrapping_mul(0x94D0_49BB_1331_11EB)
}

fn be(state: &[u8], at: usize, bytes: usize) -> Option<usize> {
    let field = state.get(at..at.checked_add(bytes)?)?;
    Some(
        field
            .iter()
            .fold(0, |value, byte| value << 8 | *byte as usize),
    )
}

fn walk(state: &[u8], parts: &mut Vec<Part>, at: usize, key: u64) -> Option<(usize, bool)> {
    let tag = *state.get(at)?;
    let (head, kids, body) = match tag {
        0x00..=0x7f | 0xc0..=0xc3 | 0xe0..=0xff => (1, 0, 0),
        0x80..=0x8f => (1, 2 * (tag as usize & 0x0f), 0),
        0x90..=0x9f => (1, tag as usize & 0x0f, 0),
        0xa0..=0xbf => (1, 0, tag as usize & 0x1f),
        0xc4 | 0xd9 => (2, 0, be(state, at + 1, 1)?),
        0xc5 | 0xda => (3, 0, be(state, at + 1, 2)?),
        0xc6 | 0xdb => (5, 0, be(state, at + 1, 4)?),
        0xc7 => (3, 0, be(state, at + 1, 1)?),
        0xc8 => (4, 0, be(state, at + 1, 2)?),
        0xc9 => (6, 0, be(state, at + 1, 4)?),
        0xcc | 0xd0 => (2, 0, 0),
        0xcd | 0xd1 => (3, 0, 0),
        0xd4 => (3, 0, 0),
        0xd5 => (4, 0, 0),
        0xca | 0xce | 0xd2 => (5, 0, 0),
        0xd6 => (6, 0, 0),
        0xcb | 0xcf | 0xd3 => (9, 0, 0),
        0xd7 => (10, 0, 0),
        0xd8 => (18, 0, 0),
        0xdc => (3, be(state, at + 1, 2)?, 0),
        0xdd => (5, be(state, at + 1, 4)?, 0),
        0xde => (3, 2 * be(state, at + 1, 2)?, 0),
        0xdf => (5, 2 * be(state, at + 1, 4)?, 0),
    };
    let mut end = at.checked_add(head)?.checked_add(body)?;
    if end > state.len() {
        return None;
    }
    let mut inner = false;
    for kid in 0..kids {
        let (kid_end, kid_inner) = walk(state, parts, end, mix(key, kid as u64))?;
        end = kid_end;
        inner |= kid_inner;
    }
    if !inner && end - at >= MIN_PART {
        parts.push(Part {
            key,
            start: at,
            len: end - at,
        });
        return Some((end, true));
    }
    Some((end, inner))
}

pub(super) fn layout(state: &[u8]) -> Vec<Part> {
    let mut big = Vec::new();
    let end = match walk(state, &mut big, 0, ROOT) {
        Some((end, _)) => end,
        None => {
            big.clear();
            0
        }
    };
    if end < state.len() {
        big.push(Part {
            key: AFTER,
            start: end,
            len: state.len() - end,
        });
    }
    let mut parts = Vec::with_capacity(big.len() * 2 + 1);
    let mut at = 0;
    for part in big {
        if part.start > at {
            parts.push(Part {
                key: mix(part.key, GAP),
                start: at,
                len: part.start - at,
            });
        }
        at = part.start + part.len;
        parts.push(part);
    }
    if at < state.len() {
        parts.push(Part {
            key: TAIL,
            start: at,
            len: state.len() - at,
        });
    }
    parts
}

pub(super) fn diff(
    old: &[u8],
    old_parts: &[Part],
    new: &[u8],
    new_parts: &[Part],
    out: &mut Vec<u8>,
) {
    let index: HashMap<u64, usize> = new_parts
        .iter()
        .enumerate()
        .map(|(at, part)| (part.key, at))
        .collect();
    out.clear();
    for part in old_parts {
        let was = &old[part.range()];
        let twin = index
            .get(&part.key)
            .filter(|at| new_parts[**at].len == part.len);
        let Some(&at) = twin else {
            out.push(RAW);
            out.extend_from_slice(&(part.len as u32).to_le_bytes());
            out.extend_from_slice(was);
            continue;
        };
        out.push(SAME);
        out.extend_from_slice(&(at as u32).to_le_bytes());
        let now = &new[new_parts[at].range()];
        for (was, now) in was.chunks(BLOCK).zip(now.chunks(BLOCK)) {
            if same(was, now) {
                out.push(SAME);
            } else {
                out.push(RAW);
                out.extend(was.iter().zip(now).map(|(was, now)| was ^ now));
            }
        }
    }
}

fn same(was: &[u8], now: &[u8]) -> bool {
    let (was_runs, was_rest) = was.as_chunks::<256>();
    let (now_runs, now_rest) = now.as_chunks::<256>();
    was.len() == now.len()
        && was_runs
            .iter()
            .zip(now_runs)
            .all(|(was, now)| same_run(was, now))
        && was_rest == now_rest
}

fn same_run(was: &[u8; 256], now: &[u8; 256]) -> bool {
    let flipped = was
        .as_chunks::<8>()
        .0
        .iter()
        .zip(now.as_chunks::<8>().0)
        .fold(0, |flips, (was, now)| {
            flips | (u64::from_ne_bytes(*was) ^ u64::from_ne_bytes(*now))
        });
    flipped == 0
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let taken = self.bytes.get(self.at..self.at.checked_add(len)?)?;
        self.at += len;
        Some(taken)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|taken| taken[0])
    }

    fn word(&mut self) -> Option<usize> {
        let taken = self.take(4)?;
        Some(u32::from_le_bytes(taken.try_into().ok()?) as usize)
    }
}

pub(super) fn undo(new: &[u8], new_parts: &[Part], delta: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let mut reader = Reader {
        bytes: delta,
        at: 0,
    };
    out.clear();
    out.reserve(new.len());
    while reader.at < delta.len() {
        match reader.byte()? {
            SAME => {
                let part = new_parts.get(reader.word()?)?;
                for now in new.get(part.range())?.chunks(BLOCK) {
                    if reader.byte()? == SAME {
                        out.extend_from_slice(now);
                    } else {
                        let flips = reader.take(now.len())?;
                        out.extend(flips.iter().zip(now).map(|(flip, now)| flip ^ now));
                    }
                }
            }
            RAW => {
                let len = reader.word()?;
                out.extend_from_slice(reader.take(len)?);
            }
            _ => return None,
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uint(out: &mut Vec<u8>, value: u32) {
        match value {
            0..=0x7f => out.push(value as u8),
            0x80..=0xffff => {
                out.push(0xcd);
                out.extend_from_slice(&(value as u16).to_be_bytes());
            }
            _ => {
                out.push(0xce);
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
    }

    fn list(out: &mut Vec<u8>, values: impl ExactSizeIterator<Item = u32>) {
        out.push(0xdd);
        out.extend_from_slice(&(values.len() as u32).to_be_bytes());
        for value in values {
            uint(out, value);
        }
    }

    fn shape(len: usize, poke: usize) -> Vec<u8> {
        let mut ram = vec![7u8; 64 * 1024];
        ram[poke] = 9;
        let mut out = vec![0x94, 0x01];
        list(&mut out, (0..len as u32).map(|at| at * 1000));
        out.push(0xc6);
        out.extend_from_slice(&(ram.len() as u32).to_be_bytes());
        out.extend_from_slice(&ram);
        list(&mut out, 0..20_000);
        out
    }

    fn round_trip(old: &[u8], new: &[u8]) -> usize {
        let (old_parts, new_parts) = (layout(old), layout(new));
        let mut delta = Vec::new();
        diff(old, &old_parts, new, &new_parts, &mut delta);
        let mut back = vec![1, 2, 3];
        undo(new, &new_parts, &delta, &mut back).unwrap();
        assert_eq!(back, old);
        delta.len()
    }

    #[test]
    fn blocks_are_the_same_only_when_every_byte_is() {
        let block: Vec<u8> = (0..BLOCK + 5).map(|at| (at * 13) as u8).collect();
        for len in [0, 3, 8, 13, 256, 300, 512, BLOCK, BLOCK + 5] {
            let was = &block[..len];
            assert!(same(was, was));
            for at in 0..len {
                let mut now = was.to_vec();
                now[at] ^= 0x40;
                assert!(!same(was, &now), "{len} {at}");
            }
        }
        assert!(!same(&block[..8], &block[..9]));
    }

    #[test]
    fn parts_cover_the_whole_state_in_order() {
        let state = shape(9000, 0);
        let parts = layout(&state);
        assert!(parts.len() > 1);
        let mut at = 0;
        for part in &parts {
            assert_eq!(part.start, at);
            at += part.len;
        }
        assert_eq!(at, state.len());
    }

    #[test]
    fn a_list_that_grows_leaves_the_parts_after_it_cheap() {
        let old = shape(9000, 10);
        let new = shape(9100, 20);
        let size = round_trip(&old, &new);
        assert!(size < 9000 * 5 + 2 * BLOCK, "{size} bytes");
    }

    #[test]
    fn bytes_that_are_not_msgpack_still_come_back() {
        let old: Vec<u8> = (0..50_000u32).map(|at| (at * 7 % 251) as u8).collect();
        let mut new = old.clone();
        new.insert(100, 3);
        round_trip(&old, &new);
        round_trip(&[], &new);
        round_trip(&new, &[]);
    }

    #[test]
    fn a_state_with_pixels_after_it_comes_back() {
        let mut old = shape(9000, 1);
        old.extend(std::iter::repeat_n(4, 92_160));
        let mut new = shape(9001, 1);
        new.extend(std::iter::repeat_n(5, 92_160));
        round_trip(&old, &new);
    }
}
