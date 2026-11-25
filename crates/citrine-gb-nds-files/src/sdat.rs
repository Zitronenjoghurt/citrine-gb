use super::{RomError, slice, u16_at, u32_at};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SoundKind {
    Sequence,
    SequenceArchive,
    Bank,
    WaveArchive,
    Stream,
}

impl SoundKind {
    pub const ALL: [SoundKind; 5] = [
        SoundKind::Sequence,
        SoundKind::SequenceArchive,
        SoundKind::Bank,
        SoundKind::WaveArchive,
        SoundKind::Stream,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SoundKind::Sequence => "Sequences",
            SoundKind::SequenceArchive => "Sequence archives",
            SoundKind::Bank => "Banks",
            SoundKind::WaveArchive => "Wave archives",
            SoundKind::Stream => "Streams",
        }
    }

    fn record(self) -> usize {
        match self {
            SoundKind::Sequence => 0,
            SoundKind::SequenceArchive => 1,
            SoundKind::Bank => 2,
            SoundKind::WaveArchive => 3,
            SoundKind::Stream => 7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundEntry {
    pub index: usize,
    pub name: Option<String>,
    pub file: usize,
    pub bank: Option<usize>,
    pub volume: u8,
    pub wave_archives: [Option<usize>; 4],
}

impl SoundEntry {
    pub fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.index.to_string())
    }
}

#[derive(Debug)]
pub struct Sdat<'a> {
    bytes: &'a [u8],
    files: Vec<(usize, usize)>,
    entries: [Vec<SoundEntry>; 5],
}

impl<'a> Sdat<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, RomError> {
        if bytes.get(..4) != Some(b"SDAT") {
            return Err(RomError::NotSdat);
        }
        let block = |at: usize, magic: &'static str| -> Result<Option<&'a [u8]>, RomError> {
            let start = u32_at(bytes, at)? as usize;
            let len = u32_at(bytes, at + 4)? as usize;
            if start == 0 || len == 0 {
                return Ok(None);
            }
            let body = slice(bytes, start, start + len)?;
            if body.get(..4) != Some(magic.as_bytes()) {
                return Err(RomError::MissingSection(magic));
            }
            Ok(Some(body))
        };
        let symb = block(0x10, "SYMB")?;
        let info = block(0x18, "INFO")?.ok_or(RomError::MissingSection("INFO"))?;
        let fat = block(0x20, "FAT ")?.ok_or(RomError::MissingSection("FAT "))?;
        let count = u32_at(fat, 8)? as usize;
        let files = (0..count)
            .map(|i| {
                let at = 12 + i * 16;
                let start = u32_at(fat, at)? as usize;
                Ok((start, start + u32_at(fat, at + 4)? as usize))
            })
            .collect::<Result<Vec<_>, RomError>>()?;
        let mut entries: [Vec<SoundEntry>; 5] = Default::default();
        let mut name_bytes = symb.map_or(0, <[u8]>::len);
        for (slot, kind) in SoundKind::ALL.into_iter().enumerate() {
            let names = match symb {
                Some(symb) => names(symb, kind, &mut name_bytes).unwrap_or_default(),
                None => Vec::new(),
            };
            entries[slot] = read_entries(info, kind, &names)?;
        }
        Ok(Self {
            bytes,
            files,
            entries,
        })
    }

    pub fn entries(&self, kind: SoundKind) -> &[SoundEntry] {
        &self.entries[SoundKind::ALL.iter().position(|&k| k == kind).unwrap_or(0)]
    }

    pub fn entry(&self, kind: SoundKind, index: usize) -> Option<&SoundEntry> {
        self.entries(kind).iter().find(|entry| entry.index == index)
    }

    pub fn entry_for_file(&self, kind: SoundKind, file: usize) -> Option<&SoundEntry> {
        self.entries(kind).iter().find(|entry| entry.file == file)
    }

    pub fn file(&self, id: usize) -> Result<&'a [u8], RomError> {
        let &(start, end) = self.files.get(id).ok_or(RomError::NoSuchId(id))?;
        slice(self.bytes, start, end)
    }
}

fn record(block: &[u8], kind: SoundKind) -> Result<(usize, usize), RomError> {
    let at = u32_at(block, 8 + kind.record() * 4)? as usize;
    if at == 0 {
        return Ok((0, 0));
    }
    Ok((at + 4, u32_at(block, at)? as usize))
}

fn names(
    symb: &[u8],
    kind: SoundKind,
    budget: &mut usize,
) -> Result<Vec<Option<String>>, RomError> {
    let (first, count) = record(symb, kind)?;
    let stride = if kind == SoundKind::SequenceArchive {
        8
    } else {
        4
    };
    (0..count)
        .map(|i| {
            let at = u32_at(symb, first + i * stride)? as usize;
            if at == 0 {
                return Ok(None);
            }
            let rest = symb.get(at..).ok_or(RomError::Truncated(at))?;
            let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            if end > *budget {
                return Ok(None);
            }
            *budget -= end;
            Ok(Some(String::from_utf8_lossy(&rest[..end]).into_owned()))
        })
        .collect()
}

fn read_entries(
    info: &[u8],
    kind: SoundKind,
    names: &[Option<String>],
) -> Result<Vec<SoundEntry>, RomError> {
    let (first, count) = record(info, kind)?;
    let mut entries = Vec::new();
    for index in 0..count {
        let at = u32_at(info, first + index * 4)? as usize;
        if at == 0 {
            continue;
        }
        let file = usize::from(u16_at(info, at)?);
        let bank = match kind {
            SoundKind::Sequence => Some(usize::from(u16_at(info, at + 4)?)),
            _ => None,
        };
        let volume = match kind {
            SoundKind::Sequence => *info.get(at + 6).ok_or(RomError::Truncated(at + 7))?,
            _ => 0x7F,
        };
        let mut wave_archives = [None; 4];
        if kind == SoundKind::Bank {
            for (slot, archive) in wave_archives.iter_mut().enumerate() {
                let id = u16_at(info, at + 4 + slot * 2)?;
                *archive = (id != 0xFFFF).then_some(usize::from(id));
            }
        }
        entries.push(SoundEntry {
            index,
            name: names.get(index).cloned().flatten(),
            file,
            bank,
            volume,
            wave_archives,
        });
    }
    Ok(entries)
}

#[cfg(any(test, feature = "fixtures"))]
pub struct FixtureSound {
    pub kind: SoundKind,
    pub name: Option<&'static str>,
    pub links: [u16; 4],
    pub bytes: Vec<u8>,
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build(sounds: &[FixtureSound]) -> Vec<u8> {
    fn block(magic: &[u8; 4], records: &[Vec<u8>]) -> Vec<u8> {
        let head = 8 + 8 * 4 + 24;
        let mut out = magic.to_vec();
        out.extend_from_slice(&[0; 4]);
        let mut body = Vec::new();
        for record in records {
            let at = head + body.len();
            out.extend_from_slice(&(at as u32).to_le_bytes());
            body.extend_from_slice(record);
        }
        out.extend_from_slice(&[0; 24]);
        out.extend_from_slice(&body);
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let len = out.len() as u32;
        out[4..8].copy_from_slice(&len.to_le_bytes());
        out
    }

    let head = 8 + 8 * 4 + 24;
    let per_record = |kind: SoundKind| -> Vec<(usize, &FixtureSound)> {
        sounds
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == kind)
            .collect()
    };
    let mut records_len = [0usize; 8];
    for kind in SoundKind::ALL {
        let stride = if kind == SoundKind::SequenceArchive {
            8
        } else {
            4
        };
        records_len[kind.record()] = 4 + per_record(kind).len() * stride;
    }
    for len in records_len.iter_mut() {
        *len = (*len).max(4);
    }
    let names_at = head + records_len.iter().sum::<usize>();
    let mut text = Vec::new();
    let mut symb_records = vec![Vec::new(); 8];
    let mut info_records = vec![Vec::new(); 8];
    let info_entries_at = head
        + (0..8)
            .map(|slot| {
                let kind = SoundKind::ALL.into_iter().find(|k| k.record() == slot);
                4 + kind.map_or(0, |kind| per_record(kind).len() * 4)
            })
            .sum::<usize>();
    let mut info_entries = Vec::new();
    for slot in 0..8 {
        let kind = SoundKind::ALL.into_iter().find(|k| k.record() == slot);
        let members = kind.map(per_record).unwrap_or_default();
        let mut symb = (members.len() as u32).to_le_bytes().to_vec();
        let mut info = symb.clone();
        for (file, sound) in members {
            let name_at = match sound.name {
                Some(name) => {
                    let at = names_at + text.len();
                    text.extend_from_slice(name.as_bytes());
                    text.push(0);
                    at as u32
                }
                None => 0,
            };
            symb.extend_from_slice(&name_at.to_le_bytes());
            if sound.kind == SoundKind::SequenceArchive {
                symb.extend_from_slice(&[0; 4]);
            }
            info.extend_from_slice(&((info_entries_at + info_entries.len()) as u32).to_le_bytes());
            info_entries.extend_from_slice(&(file as u16).to_le_bytes());
            info_entries.extend_from_slice(&[0, 0]);
            if sound.kind == SoundKind::Bank {
                sound
                    .links
                    .iter()
                    .for_each(|link| info_entries.extend_from_slice(&link.to_le_bytes()));
            } else {
                info_entries.extend_from_slice(&sound.links[0].to_le_bytes());
                info_entries.extend_from_slice(&[0x7F, 0x40, 0x40, 0, 0, 0]);
            }
        }
        symb_records[slot] = symb;
        info_records[slot] = info;
    }
    let mut symb = block(b"SYMB", &symb_records);
    symb.truncate(names_at);
    symb.extend_from_slice(&text);
    while !symb.len().is_multiple_of(4) {
        symb.push(0);
    }
    let len = symb.len() as u32;
    symb[4..8].copy_from_slice(&len.to_le_bytes());
    let mut info = block(b"INFO", &info_records);
    info.truncate(info_entries_at);
    info.extend_from_slice(&info_entries);
    let len = info.len() as u32;
    info[4..8].copy_from_slice(&len.to_le_bytes());

    let symb_at = 0x40;
    let info_at = symb_at + symb.len();
    let fat_at = info_at + info.len();
    let fat_len = 12 + sounds.len() * 16;
    let file_at = fat_at + fat_len;
    let mut fat = b"FAT ".to_vec();
    fat.extend_from_slice(&(fat_len as u32).to_le_bytes());
    fat.extend_from_slice(&(sounds.len() as u32).to_le_bytes());
    let mut data = b"FILE".to_vec();
    data.extend_from_slice(&[0; 12]);
    for sound in sounds {
        fat.extend_from_slice(&((file_at + data.len()) as u32).to_le_bytes());
        fat.extend_from_slice(&(sound.bytes.len() as u32).to_le_bytes());
        fat.extend_from_slice(&[0; 8]);
        data.extend_from_slice(&sound.bytes);
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
    }
    let data_len = data.len() as u32;
    data[4..8].copy_from_slice(&data_len.to_le_bytes());
    data[8..12].copy_from_slice(&(sounds.len() as u32).to_le_bytes());

    let mut out = b"SDAT".to_vec();
    out.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x01]);
    out.extend_from_slice(&((file_at + data.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0x40u16.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    for (at, len) in [
        (symb_at, symb.len()),
        (info_at, info.len()),
        (fat_at, fat_len),
        (file_at, data.len()),
    ] {
        out.extend_from_slice(&(at as u32).to_le_bytes());
        out.extend_from_slice(&(len as u32).to_le_bytes());
    }
    out.resize(0x40, 0);
    out.extend_from_slice(&symb);
    out.extend_from_slice(&info);
    out.extend_from_slice(&fat);
    out.extend_from_slice(&data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(kind: SoundKind, name: Option<&'static str>, magic: &[u8; 4]) -> FixtureSound {
        FixtureSound {
            kind,
            name,
            links: [1, 0xFFFF, 0xFFFF, 0xFFFF],
            bytes: [magic.as_slice(), &[0xFF, 0xFE, 0, 1, 1, 2, 3]].concat(),
        }
    }

    fn sample() -> Vec<u8> {
        build(&[
            sound(SoundKind::Sequence, Some("SEQ_TITLE"), b"SSEQ"),
            sound(SoundKind::Sequence, None, b"SSEQ"),
            sound(SoundKind::Bank, Some("BANK_TITLE"), b"SBNK"),
            sound(SoundKind::WaveArchive, Some("WAVE_TITLE"), b"SWAR"),
            sound(SoundKind::Stream, Some("STRM_INTRO"), b"STRM"),
        ])
    }

    #[test]
    fn lists_each_kind_with_names_and_files() {
        let bytes = sample();
        let sdat = Sdat::parse(&bytes).unwrap();
        let sequences = sdat.entries(SoundKind::Sequence);
        assert_eq!(sequences.len(), 2);
        assert_eq!(sequences[0].label(), "SEQ_TITLE");
        assert_eq!(sequences[1].label(), "1");
        assert_eq!(sequences[0].bank, Some(1));
        assert_eq!(sequences[0].volume, 0x7F);
        assert_eq!(
            sdat.entries(SoundKind::Bank)[0].wave_archives,
            [Some(1), None, None, None]
        );
        assert_eq!(sdat.entry(SoundKind::Sequence, 1), Some(&sequences[1]));
        assert_eq!(
            sdat.entry_for_file(SoundKind::Bank, 2)
                .map(SoundEntry::label),
            Some("BANK_TITLE".to_owned())
        );
        assert!(sdat.entries(SoundKind::SequenceArchive).is_empty());
        let stream = &sdat.entries(SoundKind::Stream)[0];
        assert_eq!(stream.label(), "STRM_INTRO");
        assert_eq!(&sdat.file(stream.file).unwrap()[..4], b"STRM");
        assert_eq!(&sdat.file(sequences[1].file).unwrap()[..4], b"SSEQ");
    }

    #[test]
    fn rejects_other_files_and_a_missing_info_block() {
        assert_eq!(Sdat::parse(b"NARC").unwrap_err(), RomError::NotSdat);
        let mut bytes = sample();
        bytes[0x18..0x20].fill(0);
        assert_eq!(
            Sdat::parse(&bytes).unwrap_err(),
            RomError::MissingSection("INFO")
        );
    }

    #[test]
    fn names_shared_past_the_symbol_block_size_are_dropped() {
        let long: &'static str = "N".repeat(200).leak();
        let mut bytes = build(&[
            sound(SoundKind::Sequence, Some(long), b"SSEQ"),
            sound(SoundKind::Sequence, Some("B"), b"SSEQ"),
            sound(SoundKind::Sequence, Some("C"), b"SSEQ"),
        ]);
        let symb = u32_at(&bytes, 0x10).unwrap() as usize;
        let pointers = symb + u32_at(&bytes, symb + 8).unwrap() as usize + 4;
        let first = bytes[pointers..pointers + 4].to_vec();
        bytes[pointers + 4..pointers + 8].copy_from_slice(&first);
        bytes[pointers + 8..pointers + 12].copy_from_slice(&first);
        let sdat = Sdat::parse(&bytes).unwrap();
        let labels: Vec<String> = sdat
            .entries(SoundKind::Sequence)
            .iter()
            .map(SoundEntry::label)
            .collect();
        assert_eq!(labels, [long, "1", "2"]);
    }

    #[test]
    fn a_file_past_the_end_is_an_error() {
        let mut bytes = sample();
        let fat = u32_at(&bytes, 0x20).unwrap() as usize;
        bytes[fat + 16..fat + 20].copy_from_slice(&0xFFFFu32.to_le_bytes());
        let sdat = Sdat::parse(&bytes).unwrap();
        assert!(sdat.file(0).is_err());
    }
}
