use crate::rom::nds::NdsHeader;
use citrine_gb_threads::{Mode, Threads};
use std::io::Read;
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RomId {
    pub crc32: u32,
    pub sha1: [u8; 20],
    pub sha256: [u8; 32],
}

impl RomId {
    pub fn new(data: &[u8]) -> Self {
        Self::hashed(data, data.len(), None).0
    }

    pub fn with_game_sha1(data: &[u8]) -> (Self, [u8; 20]) {
        Self::with_game_sha1_on(data, None)
    }

    pub fn with_game_sha1_on(data: &[u8], threads: Option<&Threads>) -> (Self, [u8; 20]) {
        Self::hashed(data, played_len(data), threads)
    }

    fn hashed(data: &[u8], played: usize, threads: Option<&Threads>) -> (Self, [u8; 20]) {
        let sha1s = || {
            let (head, tail) = data.split_at(played.min(data.len()));
            let mut sha1 = <sha1::Sha1 as sha1::Digest>::new();
            sha1::Digest::update(&mut sha1, head);
            let game: [u8; 20] = sha1::Digest::finalize(sha1.clone()).into();
            sha1::Digest::update(&mut sha1, tail);
            (sha1::Digest::finalize(sha1).into(), game)
        };
        let sha256 = || <sha2::Sha256 as sha2::Digest>::digest(data).into();
        let crc32 = || crc32fast::hash(data);
        let ((sha1, game_sha1), (crc32, sha256)) = match threads {
            Some(threads) if data.len() >= HASH_APART && threads.pool_size() > 1 => {
                threads.both("rom sha1", sha1s, || {
                    let (sha256, crc32) = threads.both("rom sha256", sha256, crc32);
                    (crc32, sha256)
                })
            }
            Some(threads) if data.len() >= HASH_APART => {
                threads.both("rom hashes", sha1s, || (crc32(), sha256()))
            }
            _ => (sha1s(), (crc32(), sha256())),
        };
        (
            Self {
                crc32,
                sha1,
                sha256,
            },
            game_sha1,
        )
    }

    pub fn filled(
        buf: &mut [u8],
        threads: &Threads,
        fill: impl FnMut(&mut [u8]) -> std::io::Result<()>,
        wanted: impl FnOnce(&[u8]) -> bool,
    ) -> std::io::Result<Option<(Self, [u8; 20])>> {
        Self::filled_on(buf, threads, threads.pool_size().min(2), fill, wanted)
    }

    fn filled_on(
        buf: &mut [u8],
        threads: &Threads,
        streams: usize,
        mut fill: impl FnMut(&mut [u8]) -> std::io::Result<()>,
        wanted: impl FnOnce(&[u8]) -> bool,
    ) -> std::io::Result<Option<(Self, [u8; 20])>> {
        let len = buf.len() as u64;
        let (head, rest) = buf.split_at_mut(buf.len().min(HASH_CHUNK));
        fill(head)?;
        if !wanted(head) {
            rest.chunks_mut(HASH_CHUNK).try_for_each(&mut fill)?;
            return Ok(None);
        }
        if threads.mode() != Mode::Threads || len < HASH_APART as u64 {
            rest.chunks_mut(HASH_CHUNK).try_for_each(&mut fill)?;
            return Ok(Some(Self::with_game_sha1_on(buf, Some(threads))));
        }
        let head: &[u8] = head;
        let (sha256_send, sha256_chunks) = mpsc::channel::<&[u8]>();
        let (sha1_send, sha1_chunks) = mpsc::channel::<&[u8]>();
        let sha256s = move || {
            let mut sha256 = <sha2::Sha256 as sha2::Digest>::new();
            for chunk in std::iter::once(head).chain(sha256_chunks) {
                sha2::Digest::update(&mut sha256, chunk);
            }
            sha2::Digest::finalize(sha256).into()
        };
        let sha1s = move || {
            let mut sha1 = <sha1::Sha1 as sha1::Digest>::new();
            let mut left = NdsHeader::played_len(head, len);
            let mut game = None;
            for chunk in std::iter::once(head).chain(sha1_chunks) {
                let take = (chunk.len() as u64).min(left) as usize;
                sha1::Digest::update(&mut sha1, &chunk[..take]);
                left -= take as u64;
                if left == 0 && game.is_none() {
                    game = Some(sha1::Digest::finalize(sha1.clone()).into());
                }
                sha1::Digest::update(&mut sha1, &chunk[take..]);
            }
            let sha1: [u8; 20] = sha1::Digest::finalize(sha1).into();
            (sha1, game.unwrap_or(sha1))
        };
        let chunks = rest.chunks_mut(HASH_CHUNK);
        let read = move || {
            let mut crc32 = crc32fast::Hasher::new();
            crc32.update(head);
            for chunk in chunks {
                fill(chunk)?;
                let chunk: &[u8] = chunk;
                crc32.update(chunk);
                let _ = sha256_send.send(chunk);
                let _ = sha1_send.send(chunk);
            }
            Ok::<_, std::io::Error>(crc32.finalize())
        };
        let here = || {
            if streams > 1 {
                let (sha1, crc32) = threads.both("rom sha1", sha1s, read);
                crc32.map(|crc32| (sha1, crc32))
            } else {
                read().map(|crc32| (sha1s(), crc32))
            }
        };
        let (sha256, here) = threads.both("rom sha256", sha256s, here);
        let ((sha1, game_sha1), crc32) = here?;
        Ok(Some((
            Self {
                crc32,
                sha1,
                sha256,
            },
            game_sha1,
        )))
    }

    pub fn crc32_hex_string(&self) -> String {
        format!("{:08X}", self.crc32)
    }

    pub fn sha1_hex_string(&self) -> String {
        hex_upper(&self.sha1)
    }

    pub fn sha256_hex_string(&self) -> String {
        hex_upper(&self.sha256)
    }
}

const HASH_CHUNK: usize = 1 << 20;
const HASH_APART: usize = 4 << 20;

fn played_len(data: &[u8]) -> usize {
    (NdsHeader::played_len(data, data.len() as u64) as usize).min(data.len())
}

pub fn rom_sha1(data: &[u8]) -> [u8; 20] {
    <sha1::Sha1 as sha1::Digest>::digest(data).into()
}

pub fn game_sha1(data: &[u8]) -> [u8; 20] {
    <sha1::Sha1 as sha1::Digest>::digest(&data[..played_len(data)]).into()
}

pub fn game_sha1_from(mut reader: impl Read, len: u64) -> std::io::Result<[u8; 20]> {
    let mut hasher = <sha1::Sha1 as sha1::Digest>::new();
    let mut chunk = vec![0; HASH_CHUNK];
    let mut left: Option<u64> = None;
    loop {
        let mut filled = 0;
        while filled < chunk.len() {
            match reader.read(&mut chunk[filled..])? {
                0 => break,
                read => filled += read,
            }
        }
        let left = left.get_or_insert_with(|| NdsHeader::played_len(&chunk[..filled], len));
        let take = (filled as u64).min(*left) as usize;
        sha1::Digest::update(&mut hasher, &chunk[..take]);
        *left -= take as u64;
        if filled < chunk.len() || *left == 0 {
            return Ok(sha1::Digest::finalize(hasher).into());
        }
    }
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_threads::Mode;

    #[test]
    fn a_trimmed_and_a_full_ds_dump_hash_alike() {
        let full = crate::rom::nds::tests::with_used(crate::rom::nds::tests::nds_rom(&[]), 0xC00);
        let trimmed = full[..0xC00].to_vec();
        assert_eq!(game_sha1(&full), game_sha1(&trimmed));
        assert_eq!(game_sha1(&trimmed), RomId::new(&trimmed).sha1);
        assert_ne!(game_sha1(&full), RomId::new(&full).sha1);
    }

    #[test]
    fn a_streamed_hash_matches_the_whole_one() {
        let ds = crate::rom::nds::tests::with_used(crate::rom::nds::tests::nds_rom(&[]), 0xC00);
        let big: Vec<u8> = (0..3 * HASH_CHUNK as u32 + 17)
            .map(|i| (i * 7) as u8)
            .collect();
        for data in [ds, big, Vec::new()] {
            let streamed = game_sha1_from(&data[..], data.len() as u64).unwrap();
            assert_eq!(streamed, game_sha1(&data));
        }
    }

    #[test]
    fn rom_ids_match_each_hash_on_its_own() {
        for (len, used) in [2 * HASH_CHUNK + 5, HASH_APART + 5]
            .into_iter()
            .flat_map(|len| [0xC00, HASH_CHUNK, HASH_CHUNK + 1, len].map(|used| (len, used)))
        {
            let mut data = crate::rom::nds::tests::nds_rom(&[]);
            data.extend((data.len()..len).map(|i| (i * 13) as u8));
            let data = crate::rom::nds::tests::with_used(data, used as u32);
            let whole = RomId {
                crc32: crc32fast::hash(&data),
                sha1: <sha1::Sha1 as sha1::Digest>::digest(&data).into(),
                sha256: <sha2::Sha256 as sha2::Digest>::digest(&data).into(),
            };
            let played: [u8; 20] = <sha1::Sha1 as sha1::Digest>::digest(&data[..used]).into();
            assert_eq!(RomId::new(&data), whole, "{used:#x}");
            assert_eq!(RomId::with_game_sha1(&data), (whole, played), "{used:#x}");
            for mode in [Mode::Threads, Mode::Inline, Mode::Shuffled(used as u64)] {
                let threads = Threads::new(mode);
                assert_eq!(
                    RomId::with_game_sha1_on(&data, Some(&threads)),
                    (whole, played),
                    "{used:#x} {mode:?}"
                );
            }
            for mode in [Mode::Threads, Mode::Inline, Mode::Shuffled(used as u64)] {
                let threads = Threads::new(mode);
                for wanted in [true, false] {
                    let mut buf = vec![0; data.len()];
                    let mut from = &data[..];
                    let ids = RomId::filled(
                        &mut buf,
                        &threads,
                        |chunk| from.read_exact(chunk),
                        |head| {
                            assert_eq!(head, &data[..head.len()]);
                            wanted
                        },
                    );
                    assert_eq!(buf, data, "{used:#x} {mode:?}");
                    assert_eq!(
                        ids.unwrap(),
                        wanted.then_some((whole, played)),
                        "{used:#x} {mode:?}"
                    );
                }
                let mut buf = vec![0; data.len()];
                let mut from = &data[..3 * HASH_CHUNK / 2];
                let short =
                    RomId::filled(&mut buf, &threads, |chunk| from.read_exact(chunk), |_| true);
                assert!(short.is_err(), "{used:#x} {mode:?}");
            }
            let threads = Threads::new(Mode::Threads);
            for streams in [1, 2] {
                let mut buf = vec![0; data.len()];
                let mut from = &data[..];
                let ids = RomId::filled_on(
                    &mut buf,
                    &threads,
                    streams,
                    |chunk| from.read_exact(chunk),
                    |_| true,
                );
                assert_eq!(buf, data, "{used:#x} {streams}");
                assert_eq!(ids.unwrap(), Some((whole, played)), "{used:#x} {streams}");
                let mut buf = vec![0; data.len()];
                let mut from = &data[..3 * HASH_CHUNK / 2];
                let short = RomId::filled_on(
                    &mut buf,
                    &threads,
                    streams,
                    |chunk| from.read_exact(chunk),
                    |_| true,
                );
                assert!(short.is_err(), "{used:#x} {streams}");
            }
            assert_eq!(game_sha1(&data), played, "{used:#x}");
            assert_eq!(rom_sha1(&data), whole.sha1, "{used:#x}");
            assert_eq!(played_len(&data), used, "{used:#x}");
        }
        assert_eq!(RomId::with_game_sha1(&[]), (RomId::new(&[]), rom_sha1(&[])));
    }

    #[test]
    fn the_hashes_match_the_fips_million_a_example() {
        let id = RomId::new(&vec![b'a'; 1_000_000]);
        assert_eq!(
            id.sha1_hex_string(),
            "34AA973CD4C4DAA4F61EEB2BDBAD27316534016F"
        );
        assert_eq!(
            id.sha256_hex_string(),
            "CDC76E5C9914FB9281A1C7E284D73E67F1809A48A497200E046D39CCC7112CD0"
        );
    }

    #[test]
    fn the_crc32_matches_the_standard_check_value() {
        assert_eq!(RomId::new(b"123456789").crc32_hex_string(), "CBF43926");
    }

    #[test]
    fn the_sha1_matches_the_fips_example() {
        assert_eq!(
            RomId::new(b"abc").sha1_hex_string(),
            "A9993E364706816ABA3E25717850C26C9CD0D89D"
        );
    }

    #[test]
    fn the_sha256_matches_the_fips_example() {
        assert_eq!(
            RomId::new(b"abc").sha256_hex_string(),
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
        );
    }

    #[test]
    fn the_header_reads_its_hashes_from_the_rom_id() {
        let mut data = vec![0; 0x8000];
        data[0x134..0x138].copy_from_slice(b"TEST");
        let header = crate::rom::header::RomHeader::new(&data).unwrap();
        assert_eq!(header.id(), RomId::new(&data));
        assert_eq!(
            header.sha256_hex_string(),
            RomId::new(&data).sha256_hex_string()
        );
    }
}
