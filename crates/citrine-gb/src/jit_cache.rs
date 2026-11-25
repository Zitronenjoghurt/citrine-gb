use crate::utils::digest::{digest, game_key};
use citrine_gb_threads::Claim;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

pub const JIT_VERSION: u32 = 21;
pub const FILE_LIMIT: usize = 64 << 20;
const MAGIC: &[u8; 4] = b"CJIT";
const BUILD: &str = env!("CITRINE_JIT_BUILD");
const VARIANTS: usize = 4;
const MAX_WORDS: usize = 256;
const MAX_CODE: usize = 1 << 20;
const SESSION: u64 = 1 << 40;
pub const LINK: u32 = 1 << 31;
pub const KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reloc {
    pub offset: u32,
    pub helper: u32,
}

impl Reloc {
    pub fn width(self) -> usize {
        if self.helper & LINK != 0 { 4 } else { 8 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CacheError {
    #[error("not a JIT cache file")]
    NotCache,
    #[error("made by another build")]
    OtherBuild,
    #[error("made for another game")]
    OtherGame,
    #[error("cut short")]
    Cut,
    #[error("damaged")]
    Damaged,
}

#[derive(Debug)]
pub struct Record {
    pub cpu: u64,
    pub key: u32,
    pub words: Box<[u32]>,
    pub fetch: Box<[[u32; 2]]>,
    pub literals: Box<[[u32; 2]]>,
    pub code: Box<[u8]>,
    pub relocs: Box<[Reloc]>,
    used: AtomicU64,
}

impl Record {
    pub fn new(
        cpu: u64,
        key: u32,
        words: &[u32],
        fetch: &[[u32; 2]],
        literals: &[[u32; 2]],
        code: &[u8],
        relocs: &[Reloc],
    ) -> Self {
        Self {
            cpu,
            key,
            words: words.into(),
            fetch: fetch.into(),
            literals: literals.into(),
            code: code.into(),
            relocs: relocs.into(),
            used: AtomicU64::new(0),
        }
    }

    fn matches(&self, words: &[u32], fetch: &[[u32; 2]], literals: &[[u32; 2]]) -> bool {
        *self.words == *words && *self.fetch == *fetch && *self.literals == *literals
    }

    fn bytes(&self) -> usize {
        22 + self.words.len() * 4
            + self.fetch.len() * 8
            + self.literals.len() * 8
            + self.relocs.len() * 8
            + self.code.len()
    }
}

#[derive(Default)]
struct Shelf {
    blocks: HashMap<(u64, u32), Vec<Arc<Record>>>,
    clock: u64,
    changed: bool,
}

impl Shelf {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        SESSION + self.clock
    }

    fn add(&mut self, record: Arc<Record>) {
        let variants = self.blocks.entry((record.cpu, record.key)).or_default();
        variants.retain(|known| !known.matches(&record.words, &record.fetch, &record.literals));
        if variants.len() >= VARIANTS
            && let Some(oldest) =
                (0..variants.len()).min_by_key(|&at| variants[at].used.load(Ordering::Relaxed))
        {
            variants.swap_remove(oldest);
        }
        variants.push(record);
    }
}

pub struct JitCache {
    game: u64,
    stamp: u64,
    key: [u8; KEY_BYTES],
    shelf: Claim<Shelf>,
}

impl JitCache {
    pub fn for_rom(rom: &[u8], key: [u8; KEY_BYTES]) -> Self {
        Self {
            game: game_key(rom),
            stamp: stamp(),
            key,
            shelf: Claim::new(Shelf::default()),
        }
    }

    pub fn file_name(&self) -> String {
        format!("{:016x}.jit", self.game)
    }

    pub fn blocks(&self) -> usize {
        self.hold(|shelf| shelf.blocks.values().map(Vec::len).sum())
    }

    pub fn take_changed(&self) -> bool {
        self.hold(|shelf| std::mem::take(&mut shelf.changed))
    }

    pub fn clear(&self) {
        self.hold(|shelf| *shelf = Shelf::default());
    }

    pub fn find(
        &self,
        cpu: u64,
        key: u32,
        words: &[u32],
        fetch: &[[u32; 2]],
        literals: &[[u32; 2]],
    ) -> Option<Arc<Record>> {
        self.hold(|shelf| {
            let found = shelf
                .blocks
                .get(&(cpu, key))?
                .iter()
                .find(|record| record.matches(words, fetch, literals))
                .cloned()?;
            let now = shelf.tick();
            if found.used.swap(now, Ordering::Relaxed) < SESSION {
                shelf.changed = true;
            }
            Some(found)
        })
    }

    pub fn keep(&self, record: Record) {
        let record = Arc::new(record);
        self.hold(|shelf| {
            record.used.store(shelf.tick(), Ordering::Relaxed);
            shelf.add(Arc::clone(&record));
            shelf.changed = true;
        });
    }

    pub fn merge(&self, bytes: &[u8]) -> Result<usize, CacheError> {
        let records = self.decode(bytes)?;
        let count = records.len();
        let records: Vec<Arc<Record>> = records
            .into_iter()
            .enumerate()
            .map(|(rank, record)| {
                record.used.store((count - rank) as u64, Ordering::Relaxed);
                Arc::new(record)
            })
            .collect();
        self.hold(|shelf| {
            for record in &records {
                let known = shelf.blocks.get(&(record.cpu, record.key));
                if !known.is_some_and(|known| {
                    known
                        .iter()
                        .any(|k| k.matches(&record.words, &record.fetch, &record.literals))
                }) {
                    shelf.add(Arc::clone(record));
                }
            }
        });
        Ok(count)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut records: Vec<Arc<Record>> =
            self.hold(|shelf| shelf.blocks.values().flatten().cloned().collect());
        records.sort_by_key(|record| std::cmp::Reverse(record.used.load(Ordering::Relaxed)));
        let mut out = Vec::with_capacity(1 << 20);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&JIT_VERSION.to_le_bytes());
        out.extend_from_slice(&self.stamp.to_le_bytes());
        out.extend_from_slice(&self.game.to_le_bytes());
        let count_at = out.len();
        out.extend_from_slice(&0u32.to_le_bytes());
        let mut count = 0u32;
        for record in records {
            if out.len() + record.bytes() + 8 > FILE_LIMIT {
                break;
            }
            write_record(&mut out, &record);
            count += 1;
        }
        out[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
        let tag = mac(&self.key, &out);
        out.extend_from_slice(&tag);
        out
    }

    fn decode(&self, bytes: &[u8]) -> Result<Vec<Record>, CacheError> {
        if !bytes.starts_with(MAGIC) {
            return Err(CacheError::NotCache);
        }
        let (body, tag) = bytes
            .split_at_checked(bytes.len().saturating_sub(KEY_BYTES))
            .ok_or(CacheError::Cut)?;
        let mut cursor = Cursor { bytes: body, at: 4 };
        let version = cursor.u32().ok_or(CacheError::Cut)?;
        let stamp = cursor.u64().ok_or(CacheError::Cut)?;
        if version != JIT_VERSION || stamp != self.stamp {
            return Err(CacheError::OtherBuild);
        }
        if cursor.u64().ok_or(CacheError::Cut)? != self.game {
            return Err(CacheError::OtherGame);
        }
        if !same(&mac(&self.key, body), tag) {
            return Err(CacheError::Damaged);
        }
        let count = cursor.u32().ok_or(CacheError::Cut)?;
        let mut records = Vec::new();
        for _ in 0..count {
            records.push(read_record(&mut cursor).ok_or(CacheError::Damaged)?);
        }
        if cursor.at != body.len() {
            return Err(CacheError::Damaged);
        }
        Ok(records)
    }

    fn hold<R>(&self, mut f: impl FnMut(&mut Shelf) -> R) -> R {
        loop {
            if let Some(result) = self.shelf.try_with(&mut f) {
                return result;
            }
            std::thread::yield_now();
        }
    }
}

fn write_record(out: &mut Vec<u8>, record: &Record) {
    out.extend_from_slice(&record.cpu.to_le_bytes());
    out.extend_from_slice(&record.key.to_le_bytes());
    out.extend_from_slice(&(record.words.len() as u16).to_le_bytes());
    out.extend_from_slice(&(record.relocs.len() as u16).to_le_bytes());
    out.extend_from_slice(&(record.literals.len() as u16).to_le_bytes());
    out.extend_from_slice(&(record.code.len() as u32).to_le_bytes());
    for word in &record.words {
        out.extend_from_slice(&word.to_le_bytes());
    }
    for pair in &record.fetch {
        for cycles in pair {
            out.extend_from_slice(&cycles.to_le_bytes());
        }
    }
    for pair in &record.literals {
        for part in pair {
            out.extend_from_slice(&part.to_le_bytes());
        }
    }
    for reloc in &record.relocs {
        out.extend_from_slice(&reloc.offset.to_le_bytes());
        out.extend_from_slice(&reloc.helper.to_le_bytes());
    }
    out.extend_from_slice(&record.code);
}

fn read_record(cursor: &mut Cursor) -> Option<Record> {
    let cpu = cursor.u64()?;
    let key = cursor.u32()?;
    let words = usize::from(cursor.u16()?);
    let relocs = usize::from(cursor.u16()?);
    let literals = usize::from(cursor.u16()?);
    let code = cursor.u32()? as usize;
    if !(3..=MAX_WORDS).contains(&words) || literals > words - 2 || code > MAX_CODE {
        return None;
    }
    let words = (0..words)
        .map(|_| cursor.u32())
        .collect::<Option<Box<[u32]>>>()?;
    let fetch = (0..words.len() - 2)
        .map(|_| Some([cursor.u32()?, cursor.u32()?]))
        .collect::<Option<Box<[[u32; 2]]>>>()?;
    let literals = (0..literals)
        .map(|_| Some([cursor.u32()?, cursor.u32()?]))
        .collect::<Option<Box<[[u32; 2]]>>>()?;
    let ops = words.len() as u32 - 2;
    if literals.iter().any(|&[index, _]| index >= ops)
        || literals.windows(2).any(|pair| pair[0][0] >= pair[1][0])
    {
        return None;
    }
    let relocs = (0..relocs)
        .map(|_| {
            Some(Reloc {
                offset: cursor.u32()?,
                helper: cursor.u32()?,
            })
        })
        .collect::<Option<Box<[Reloc]>>>()?;
    if relocs
        .iter()
        .any(|reloc| reloc.offset as usize + reloc.width() > code)
    {
        return None;
    }
    let code = cursor.take(code)?.into();
    Some(Record {
        cpu,
        key,
        words,
        fetch,
        literals,
        code,
        relocs,
        used: AtomicU64::new(0),
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(len)?)?;
        self.at += len;
        Some(slice)
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2)?.try_into().ok().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)?.try_into().ok().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take(8)?.try_into().ok().map(u64::from_le_bytes)
    }
}

fn mac(key: &[u8], body: &[u8]) -> [u8; KEY_BYTES] {
    let pad = |byte: u8| {
        let mut block = [byte; 64];
        for (slot, key) in block.iter_mut().zip(key) {
            *slot ^= key;
        }
        block
    };
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(body)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5C))
        .chain_update(inner)
        .finalize()
        .into()
}

fn same(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0, |differ, (left, right)| differ | (left ^ right))
            == 0
}

pub fn cpu_tag(name: &str, v5: bool) -> u64 {
    digest(format!("{name} {v5}").as_bytes())
}

fn stamp() -> u64 {
    static STAMP: OnceLock<u64> = OnceLock::new();
    *STAMP.get_or_init(|| {
        let target = target();
        digest(format!("{BUILD} {JIT_VERSION} {target}").as_bytes())
    })
}

#[cfg(jit)]
fn target() -> String {
    crate::jit::target()
}

#[cfg(not(jit))]
fn target() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROM: &[u8] = &[7; 0x400];
    const KEY: [u8; KEY_BYTES] = [3; KEY_BYTES];
    const WORDS: &[u32] = &[1, 2, 3, 4];
    const FETCH: &[[u32; 2]] = &[[1, 2], [1, 2]];
    const LITERALS: &[[u32; 2]] = &[[0, 0xDEAD_BEEF], [1, 7]];

    fn record(key: u32, word: u32) -> Record {
        Record::new(
            9,
            key,
            &[word, 2, 3],
            &[[1, 2]],
            &[],
            &[0xAA; 16],
            &[Reloc {
                offset: 4,
                helper: 1,
            }],
        )
    }

    fn filled() -> JitCache {
        let cache = JitCache::for_rom(ROM, KEY);
        cache.keep(record(0x200_0000, 1));
        cache.keep(record(0x200_0041, 5));
        cache
    }

    fn literal_record(key: u32, literals: &[[u32; 2]], code: u8) -> Record {
        Record::new(9, key, WORDS, FETCH, literals, &[code; 16], &[])
    }

    fn reloaded(literals: &[[u32; 2]]) -> Result<usize, CacheError> {
        let cache = JitCache::for_rom(ROM, KEY);
        cache.keep(literal_record(0x800_0101, literals, 0xAA));
        JitCache::for_rom(ROM, KEY).merge(&cache.encode())
    }

    #[test]
    fn a_saved_cache_loads_every_block_back() {
        let cache = filled();
        cache.keep(literal_record(0x800_0081, LITERALS, 0xBB));
        let bytes = cache.encode();
        let loaded = JitCache::for_rom(ROM, KEY);
        assert_eq!(loaded.merge(&bytes), Ok(3));
        let found = loaded
            .find(9, 0x800_0081, WORDS, FETCH, LITERALS)
            .expect("the block with literals loads");
        assert_eq!(*found.literals, *LITERALS);
        assert_eq!(*found.code, [0xBB; 16]);
        assert!(loaded.find(9, 0x800_0081, WORDS, FETCH, &[]).is_none());
        let found = loaded
            .find(9, 0x200_0041, &[5, 2, 3], &[[1, 2]], &[])
            .expect("the block loads");
        assert_eq!(*found.code, [0xAA; 16]);
        assert_eq!(
            *found.relocs,
            [Reloc {
                offset: 4,
                helper: 1
            }]
        );
        assert!(loaded.take_changed(), "a used block moves to the front");
    }

    #[test]
    fn a_link_in_the_last_word_of_a_block_loads_back() {
        let cache = JitCache::for_rom(ROM, KEY);
        let link = Reloc {
            offset: 12,
            helper: LINK | 3,
        };
        cache.keep(Record::new(
            9,
            0x200_0000,
            &[1, 2, 3],
            &[[1, 2]],
            &[],
            &[0xAA; 16],
            &[link],
        ));
        let loaded = JitCache::for_rom(ROM, KEY);
        assert_eq!(loaded.merge(&cache.encode()), Ok(1));
        let found = loaded
            .find(9, 0x200_0000, &[1, 2, 3], &[[1, 2]], &[])
            .expect("the block loads");
        assert_eq!(*found.relocs, [link]);
    }

    #[test]
    fn a_block_is_found_only_when_its_code_and_timing_match() {
        let cache = filled();
        assert!(
            cache
                .find(9, 0x200_0000, &[1, 2, 3], &[[1, 2]], &[])
                .is_some()
        );
        assert!(
            cache
                .find(9, 0x200_0000, &[1, 2, 4], &[[1, 2]], &[])
                .is_none()
        );
        assert!(
            cache
                .find(9, 0x200_0000, &[1, 2, 3], &[[1, 3]], &[])
                .is_none()
        );
        assert!(
            cache
                .find(8, 0x200_0000, &[1, 2, 3], &[[1, 2]], &[])
                .is_none()
        );
        assert!(
            cache
                .find(9, 0x200_0001, &[1, 2, 3], &[[1, 2]], &[])
                .is_none()
        );
    }

    #[test]
    fn a_block_is_found_only_when_its_literals_match() {
        let cache = JitCache::for_rom(ROM, KEY);
        cache.keep(literal_record(0x800_0101, &[[0, 0x1234_5678]], 0xAA));
        assert!(
            cache
                .find(9, 0x800_0101, WORDS, FETCH, &[[0, 0x1234_5678]])
                .is_some()
        );
        assert!(
            cache
                .find(9, 0x800_0101, WORDS, FETCH, &[[0, 0x1234_565A]])
                .is_none()
        );
        assert!(
            cache
                .find(9, 0x800_0101, WORDS, FETCH, &[[1, 0x1234_5678]])
                .is_none()
        );
        assert!(cache.find(9, 0x800_0101, WORDS, FETCH, &[]).is_none());
        cache.keep(literal_record(0x800_0101, &[[0, 0x1234_565A]], 0xBB));
        assert_eq!(cache.blocks(), 2);
        let found = cache
            .find(9, 0x800_0101, WORDS, FETCH, &[[0, 0x1234_565A]])
            .expect("the patched block is kept beside the first");
        assert_eq!(*found.code, [0xBB; 16]);
        let found = cache
            .find(9, 0x800_0101, WORDS, FETCH, &[[0, 0x1234_5678]])
            .expect("the first block stays");
        assert_eq!(*found.code, [0xAA; 16]);
    }

    #[test]
    fn damaged_literals_are_refused() {
        assert_eq!(reloaded(LITERALS), Ok(1));
        assert_eq!(reloaded(&[[2, 5]]), Err(CacheError::Damaged));
        assert_eq!(reloaded(&[[1, 5], [0, 6]]), Err(CacheError::Damaged));
        assert_eq!(reloaded(&[[1, 5], [1, 6]]), Err(CacheError::Damaged));
        let cache = JitCache::for_rom(ROM, KEY);
        cache.keep(literal_record(0x800_0101, &[[0, 5]], 0xAA));
        let mut bytes = cache.encode();
        let body = bytes.len() - KEY_BYTES;
        assert_eq!(bytes[body..], mac(&KEY, &bytes[..body]));
        assert_eq!(bytes[44..46], 1u16.to_le_bytes());
        bytes[44..46].copy_from_slice(&3u16.to_le_bytes());
        let tag = mac(&KEY, &bytes[..body]);
        bytes[body..].copy_from_slice(&tag);
        assert_eq!(
            JitCache::for_rom(ROM, KEY).merge(&bytes),
            Err(CacheError::Damaged)
        );
    }

    #[test]
    fn a_cut_or_damaged_file_is_refused() {
        let bytes = filled().encode();
        let cache = JitCache::for_rom(ROM, KEY);
        assert_eq!(
            cache.merge(&bytes[..bytes.len() - 3]),
            Err(CacheError::Damaged)
        );
        assert_eq!(cache.merge(&bytes[..10]), Err(CacheError::Cut));
        let mut flipped = bytes.clone();
        flipped[40] ^= 1;
        assert_eq!(cache.merge(&flipped), Err(CacheError::Damaged));
        assert_eq!(cache.merge(b"nope"), Err(CacheError::NotCache));
        assert_eq!(cache.blocks(), 0);
    }

    #[test]
    fn a_file_signed_with_another_key_is_refused() {
        let bytes = filled().encode();
        let other = JitCache::for_rom(ROM, [4; KEY_BYTES]);
        assert_eq!(other.merge(&bytes), Err(CacheError::Damaged));
        assert_eq!(other.blocks(), 0);
        assert_eq!(JitCache::for_rom(ROM, KEY).merge(&bytes), Ok(2));
    }

    #[test]
    fn files_are_signed_with_hmac_sha256() {
        let hex = |tag: [u8; KEY_BYTES]| tag.map(|byte| format!("{byte:02x}")).concat();
        assert_eq!(
            hex(mac(&[0x0B; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hex(mac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn another_game_or_build_starts_empty() {
        let bytes = filled().encode();
        let other = JitCache::for_rom(&[8; 0x400], KEY);
        assert_eq!(other.merge(&bytes), Err(CacheError::OtherGame));
        let mut older = bytes.clone();
        older[8] ^= 1;
        assert_eq!(
            JitCache::for_rom(ROM, KEY).merge(&older),
            Err(CacheError::OtherBuild)
        );
        let mut version = bytes;
        version[4] ^= 1;
        assert_eq!(
            JitCache::for_rom(ROM, KEY).merge(&version),
            Err(CacheError::OtherBuild)
        );
    }

    #[test]
    fn recently_used_blocks_are_saved_first() {
        let cache = JitCache::for_rom(ROM, KEY);
        for key in 0..4 {
            cache.keep(record(key * 4, key));
        }
        cache.find(9, 0, &[0, 2, 3], &[[1, 2]], &[]);
        let loaded = JitCache::for_rom(ROM, KEY);
        loaded.merge(&cache.encode()).expect("the file loads");
        let mut order: Vec<(u64, u32)> = loaded.hold(|shelf| {
            shelf
                .blocks
                .values()
                .flatten()
                .map(|record| (record.used.load(Ordering::Relaxed), record.key))
                .collect()
        });
        order.sort_by_key(|&(used, _)| std::cmp::Reverse(used));
        let keys: Vec<u32> = order.into_iter().map(|(_, key)| key).collect();
        assert_eq!(keys, [0, 12, 8, 4]);
    }

    #[test]
    fn a_key_keeps_only_its_newest_variants() {
        let cache = JitCache::for_rom(ROM, KEY);
        for word in 0..VARIANTS as u32 + 2 {
            cache.keep(record(0, word));
        }
        assert_eq!(cache.blocks(), VARIANTS);
        assert!(cache.find(9, 0, &[0, 2, 3], &[[1, 2]], &[]).is_none());
        let newest = VARIANTS as u32 + 1;
        assert!(cache.find(9, 0, &[newest, 2, 3], &[[1, 2]], &[]).is_some());
    }
}
