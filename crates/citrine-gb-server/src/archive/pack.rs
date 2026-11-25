use citrine_gb_gameinfo::header::HEAD_BYTES;
use citrine_gb_net::{ARCHIVE_PART_BYTES, ARCHIVE_PREFIX_BYTES, ArchiveKey};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

const LEVEL: i32 = 19;
const READ_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hashes {
    pub size: u64,
    pub sha256: [u8; 32],
    pub sha1: [u8; 20],
    pub crc32: u32,
    pub head: Vec<u8>,
}

pub fn hash(path: &Path) -> io::Result<Hashes> {
    let mut file = File::open(path)?;
    let mut sha256 = Sha256::new();
    let mut sha1 = Sha1::new();
    let mut crc32 = crc32fast::Hasher::new();
    let mut size = 0;
    let mut buf = vec![0; READ_BYTES];
    let mut head = Vec::new();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let room = HEAD_BYTES.saturating_sub(head.len()).min(n);
        head.extend_from_slice(&buf[..room]);
        sha256.update(&buf[..n]);
        sha1.update(&buf[..n]);
        crc32.update(&buf[..n]);
        size += n as u64;
    }
    Ok(Hashes {
        size,
        sha256: sha256.finalize().into(),
        sha1: sha1.finalize().into(),
        crc32: crc32.finalize(),
        head,
    })
}

struct Parts<W: Write> {
    out: W,
    key: ArchiveKey,
    prefix: [u8; ARCHIVE_PREFIX_BYTES],
    buf: Vec<u8>,
    index: u32,
    written: u64,
}

impl<W: Write> Parts<W> {
    fn seal(&mut self, len: usize) -> io::Result<()> {
        let sealed = self
            .key
            .seal_part(&self.prefix, self.index, &self.buf[..len]);
        self.out.write_all(&sealed)?;
        self.buf.drain(..len);
        self.index += 1;
        self.written += sealed.len() as u64;
        Ok(())
    }

    fn finish(mut self) -> io::Result<(W, u64)> {
        if !self.buf.is_empty() {
            self.seal(self.buf.len())?;
        }
        self.out.flush()?;
        Ok((self.out, self.written))
    }
}

impl<W: Write> Write for Parts<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        while self.buf.len() >= ARCHIVE_PART_BYTES {
            self.seal(ARCHIVE_PART_BYTES)?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn pack(
    src: &Path,
    dst: &Path,
    key: &ArchiveKey,
    prefix: &[u8; ARCHIVE_PREFIX_BYTES],
    mut progress: impl FnMut(u64) -> bool,
) -> io::Result<u64> {
    let mut input = File::open(src)?;
    let size = input.metadata()?.len();
    let out = File::options().write(true).create_new(true).open(dst)?;
    let parts = Parts {
        out: BufWriter::new(out),
        key: key.clone(),
        prefix: *prefix,
        buf: Vec::with_capacity(ARCHIVE_PART_BYTES * 2),
        index: 0,
        written: 0,
    };
    let mut encoder = zstd::stream::write::Encoder::new(parts, LEVEL)?;
    encoder.set_pledged_src_size(Some(size))?;
    encoder.include_contentsize(true)?;
    encoder.include_checksum(true)?;
    let mut buf = vec![0; READ_BYTES];
    let mut done = 0;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        encoder.write_all(&buf[..n])?;
        done += n as u64;
        if !progress(done) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
        }
    }
    let (out, written) = encoder.finish()?.finish()?;
    out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_net::{ARCHIVE_SEALED_PART_BYTES, archive_parts};

    fn unpack(sealed: &[u8], key: &ArchiveKey, prefix: &[u8; ARCHIVE_PREFIX_BYTES]) -> Vec<u8> {
        let mut packed = Vec::new();
        for (i, part) in sealed.chunks(ARCHIVE_SEALED_PART_BYTES).enumerate() {
            packed.extend(key.open_part(prefix, i as u32, part).unwrap());
        }
        zstd::stream::decode_all(packed.as_slice()).unwrap()
    }

    fn rom(len: usize) -> Vec<u8> {
        let mut x = 0x1234_5678u32;
        (0..len)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if i % 4096 < 2048 { 0xFF } else { x as u8 }
            })
            .collect()
    }

    #[test]
    fn a_packed_game_unpacks_to_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("game.gba");
        let data = rom(3 * ARCHIVE_PART_BYTES + 77);
        std::fs::write(&src, &data).unwrap();
        let key = ArchiveKey([4; 32]);
        let prefix = [6; ARCHIVE_PREFIX_BYTES];
        let dst = dir.path().join("out");
        let mut seen = 0;
        let stored = pack(&src, &dst, &key, &prefix, |n| {
            seen = n;
            true
        })
        .unwrap();
        let sealed = std::fs::read(&dst).unwrap();
        assert_eq!(stored, sealed.len() as u64);
        assert_eq!(seen, data.len() as u64);
        assert!(stored < data.len() as u64);
        assert!(archive_parts(stored) >= 1);
        assert_eq!(unpack(&sealed, &key, &prefix), data);
    }

    #[test]
    fn a_progress_that_says_stop_stops_the_pack() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("game.gba");
        std::fs::write(&src, rom(3 * READ_BYTES)).unwrap();
        let mut calls = 0;
        let packed = pack(
            &src,
            &dir.path().join("out"),
            &ArchiveKey([4; 32]),
            &[0; ARCHIVE_PREFIX_BYTES],
            |_| {
                calls += 1;
                false
            },
        );
        assert_eq!(packed.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(calls, 1);
    }

    #[test]
    fn hashes_match_one_shot_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("game.gb");
        let data = rom(READ_BYTES + 5);
        std::fs::write(&src, &data).unwrap();
        let h = hash(&src).unwrap();
        assert_eq!(h.size, data.len() as u64);
        assert_eq!(h.sha256, <[u8; 32]>::from(Sha256::digest(&data)));
        assert_eq!(h.sha1, <[u8; 20]>::from(Sha1::digest(&data)));
        assert_eq!(h.crc32, crc32fast::hash(&data));
    }

    #[test]
    fn pack_never_overwrites_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("game.gb");
        std::fs::write(&src, rom(100)).unwrap();
        let dst = dir.path().join("out");
        std::fs::write(&dst, b"keep").unwrap();
        let key = ArchiveKey([4; 32]);
        assert!(pack(&src, &dst, &key, &[0; ARCHIVE_PREFIX_BYTES], |_| true).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(), b"keep");
    }
}
