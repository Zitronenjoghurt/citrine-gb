mod bank;
mod blz;
mod g2d;
mod kind;
mod mdl0;
mod narc;
mod nds;
mod sdat;
mod sseq;
mod tex0;
mod texture;
mod tree;
mod wave;

pub use bank::{Instrument, NoteDef, Sbnk, Voice};
pub use g2d::{Depth, Ncgr, Nclr, Nscr, Picture};
pub use kind::{FileType, Group};
pub use mdl0::{IDENTITY, Material, Matrix, Model, Step, models};
pub use narc::Narc;
pub use nds::NdsRom;
pub use sdat::{Sdat, SoundEntry, SoundKind};
pub use sseq::sequence_data;
pub use tex0::{PaletteInfo, Tex0, TextureInfo, Vram};
pub use texture::{Texels, TextureFormat, palette_rgba, rgb555};
pub use tree::{FileTree, Node, NodeId, Query, Row, Sort, Source};
pub use wave::{Swar, Wave, WaveFormat};

#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures {
    pub mod g2d {
        pub use crate::g2d::{build_ncgr, build_nclr, build_nscr};
    }

    pub mod mdl0 {
        pub use crate::mdl0::build_triangle;
    }

    pub mod narc {
        pub use crate::narc::build;
    }

    pub mod sdat {
        pub use crate::sdat::{FixtureSound, build};
    }

    pub mod bank {
        pub use crate::bank::build;
    }

    pub mod sseq {
        pub use crate::sseq::build;
    }

    pub mod wave {
        pub use crate::wave::{build_info, build_swar, build_swav};
    }

    pub mod tex0 {
        pub use crate::tex0::{FixtureTexture, build};
    }

    pub mod nds {
        pub use crate::nds::{arm9_with_params, build, build_with_arm9};
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RomError {
    #[error("the file ends before offset {0:#x}")]
    Truncated(usize),
    #[error("not a NARC archive")]
    NotNarc,
    #[error("not a sound archive")]
    NotSdat,
    #[error("this isn't a valid {0} file")]
    NotSound(&'static str),
    #[error("the wave format is unknown")]
    BadWave,
    #[error("missing NARC section {0}")]
    MissingSection(&'static str),
    #[error("no file at {0}")]
    NoSuchFile(String),
    #[error("no file with id {0}")]
    NoSuchId(usize),
    #[error("a file name table entry is broken at {0:#x}")]
    BadNameTable(usize),
    #[error("the compressed arm9 code is broken at {0:#x}")]
    BadCompression(usize),
    #[error("the arm9 code has no module parameters")]
    NoModuleParams,
    #[error("unknown model render step at {0:#x}")]
    BadRenderStep(usize),
    #[error("a {0} x {1} picture is too large")]
    TooLarge(usize, usize),
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, RomError> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(RomError::Truncated(at + 2))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, RomError> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(RomError::Truncated(at + 4))
}

#[cfg(any(test, feature = "fixtures"))]
fn sound_header(magic: &[u8; 4], body: usize) -> Vec<u8> {
    let mut out = magic.to_vec();
    out.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x01]);
    out.extend_from_slice(&((0x10 + body) as u32).to_le_bytes());
    out.extend_from_slice(&0x10u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out
}

fn slice(bytes: &[u8], start: usize, end: usize) -> Result<&[u8], RomError> {
    if start > end {
        return Err(RomError::Truncated(start));
    }
    bytes.get(start..end).ok_or(RomError::Truncated(end))
}

#[derive(Debug, Clone, Copy)]
struct FileRange {
    start: usize,
    end: usize,
}

fn file_ranges(fat: &[u8], base: usize) -> Result<Vec<FileRange>, RomError> {
    (0..fat.len() / 8)
        .map(|i| {
            Ok(FileRange {
                start: base + u32_at(fat, i * 8)? as usize,
                end: base + u32_at(fat, i * 8 + 4)? as usize,
            })
        })
        .collect()
}

const MAX_PATHS: usize = 0x10000;
const MAX_PATH_BYTES: usize = 1 << 24;

struct NameWalk<'a> {
    fnt: &'a [u8],
    paths: Vec<(String, usize)>,
    seen: Vec<bool>,
    bytes: usize,
}

fn name_table(fnt: &[u8]) -> Result<Vec<(String, usize)>, RomError> {
    let mut walk = NameWalk {
        fnt,
        paths: Vec::new(),
        seen: vec![false; 0x1000],
        bytes: 0,
    };
    walk.seen[0] = true;
    walk.dir(0, String::new(), 0)?;
    Ok(walk.paths)
}

impl NameWalk<'_> {
    fn dir(&mut self, dir: usize, prefix: String, depth: usize) -> Result<(), RomError> {
        if depth > 64 {
            return Err(RomError::BadNameTable(dir * 8));
        }
        let fnt = self.fnt;
        let mut at = u32_at(fnt, dir * 8)? as usize;
        let mut id = usize::from(u16_at(fnt, dir * 8 + 4)?);
        loop {
            let kind = *fnt.get(at).ok_or(RomError::Truncated(at + 1))?;
            if kind == 0 {
                return Ok(());
            }
            let len = usize::from(kind & 0x7F);
            let name = slice(fnt, at + 1, at + 1 + len)?;
            let path = format!("{prefix}{}", String::from_utf8_lossy(name));
            at += 1 + len;
            if kind & 0x80 == 0 {
                self.bytes += path.len();
                if self.paths.len() >= MAX_PATHS || self.bytes > MAX_PATH_BYTES {
                    return Err(RomError::BadNameTable(at - 1 - len));
                }
                self.paths.push((path, id));
                id += 1;
            } else {
                let child = u16_at(fnt, at)?;
                at += 2;
                if child < 0xF000 {
                    return Err(RomError::BadNameTable(at - 2));
                }
                let child = usize::from(child & 0x0FFF);
                if std::mem::replace(&mut self.seen[child], true) {
                    return Err(RomError::BadNameTable(at - 2));
                }
                self.dir(child, format!("{path}/"), depth + 1)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnt(dirs: &[Vec<u8>]) -> Vec<u8> {
        let mut main = Vec::new();
        let mut subs = Vec::new();
        for entries in dirs {
            main.extend_from_slice(&((dirs.len() * 8 + subs.len()) as u32).to_le_bytes());
            main.extend_from_slice(&[0, 0, 0, 0xF0]);
            subs.extend_from_slice(entries);
            subs.push(0);
        }
        [main, subs].concat()
    }

    #[test]
    fn a_folder_reached_twice_is_a_broken_name_table() {
        let mut dirs: Vec<Vec<u8>> = (1..64u16)
            .map(|next| {
                let link = (0xF000 | next).to_le_bytes();
                [
                    [0x81, b'a', link[0], link[1]],
                    [0x81, b'b', link[0], link[1]],
                ]
                .concat()
            })
            .collect();
        dirs.push(vec![0x01, b'f']);
        assert!(matches!(
            name_table(&fnt(&dirs)),
            Err(RomError::BadNameTable(_))
        ));
    }

    #[test]
    fn a_tree_of_folders_lists_every_file_once() {
        let dirs = [
            vec![0x81, b'a', 0x01, 0xF0, 0x81, b'b', 0x02, 0xF0],
            vec![0x01, b'x'],
            vec![0x01, b'y', 0x01, b'z'],
        ];
        let paths = name_table(&fnt(&dirs)).unwrap();
        assert_eq!(
            paths,
            [
                ("a/x".to_owned(), 0),
                ("b/y".to_owned(), 0),
                ("b/z".to_owned(), 1)
            ]
        );
    }

    #[test]
    fn too_many_files_is_a_broken_name_table() {
        let root: Vec<u8> = (0..=MAX_PATHS).flat_map(|_| [0x01, b'f']).collect();
        assert!(matches!(
            name_table(&fnt(&[root])),
            Err(RomError::BadNameTable(_))
        ));
    }
}
