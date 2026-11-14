use super::{RomError, slice, u16_at, u32_at};

const COUNT: usize = 0x38;
const NOTE_BYTES: usize = 10;
const REGION_BYTES: usize = 2 + NOTE_BYTES;
const DRUM_SET: u8 = 16;
const KEY_SPLIT: u8 = 17;
const SPLITS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Pcm { wave: u16, archive: u16 },
    Square { duty: u8 },
    Noise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteDef {
    pub voice: Voice,
    pub base_note: u8,
    pub attack: u8,
    pub decay: u8,
    pub sustain: u8,
    pub release: u8,
    pub pan: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instrument {
    Single(NoteDef),
    Drums {
        low: u8,
        keys: Vec<Option<NoteDef>>,
    },
    Split {
        tops: Vec<u8>,
        regions: Vec<Option<NoteDef>>,
    },
}

impl Instrument {
    pub fn note(&self, key: u8) -> Option<NoteDef> {
        match self {
            Instrument::Single(def) => Some(*def),
            Instrument::Drums { low, keys } => {
                keys.get(usize::from(key.checked_sub(*low)?)).copied()?
            }
            Instrument::Split { tops, regions } => {
                let region = tops.iter().position(|&top| key <= top)?;
                regions.get(region).copied()?
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sbnk {
    instruments: Vec<Option<Instrument>>,
}

impl Sbnk {
    pub fn parse(bytes: &[u8]) -> Result<Self, RomError> {
        if bytes.get(..4) != Some(b"SBNK") {
            return Err(RomError::NotSound("SBNK"));
        }
        let count = u32_at(bytes, COUNT)? as usize;
        let instruments = (0..count)
            .map(|i| {
                let at = COUNT + 4 + i * 4;
                let kind = *bytes.get(at).ok_or(RomError::Truncated(at + 1))?;
                let offset = usize::from(u16_at(bytes, at + 1)?);
                instrument(bytes, kind, offset)
            })
            .collect::<Result<Vec<_>, RomError>>()?;
        Ok(Self { instruments })
    }

    pub fn len(&self) -> usize {
        self.instruments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instruments.is_empty()
    }

    pub fn instrument(&self, program: usize) -> Option<&Instrument> {
        self.instruments.get(program)?.as_ref()
    }
}

fn instrument(bytes: &[u8], kind: u8, at: usize) -> Result<Option<Instrument>, RomError> {
    Ok(match kind {
        0 => None,
        DRUM_SET => {
            let low = *bytes.get(at).ok_or(RomError::Truncated(at + 1))?;
            let high = *bytes.get(at + 1).ok_or(RomError::Truncated(at + 2))?;
            let keys = (0..usize::from(high.saturating_sub(low)) + 1)
                .map(|i| region(bytes, at + 2 + i * REGION_BYTES))
                .collect::<Result<_, _>>()?;
            Some(Instrument::Drums { low, keys })
        }
        KEY_SPLIT => {
            let tops: Vec<u8> = slice(bytes, at, at + SPLITS)?
                .iter()
                .copied()
                .take_while(|&top| top != 0)
                .collect();
            let regions = (0..tops.len())
                .map(|i| region(bytes, at + SPLITS + i * REGION_BYTES))
                .collect::<Result<_, _>>()?;
            Some(Instrument::Split { tops, regions })
        }
        kind => note(bytes, kind, at)?.map(Instrument::Single),
    })
}

fn region(bytes: &[u8], at: usize) -> Result<Option<NoteDef>, RomError> {
    let kind = *bytes.get(at).ok_or(RomError::Truncated(at + 1))?;
    note(bytes, kind, at + 2)
}

fn note(bytes: &[u8], kind: u8, at: usize) -> Result<Option<NoteDef>, RomError> {
    let body = slice(bytes, at, at + NOTE_BYTES)?;
    let first = u16::from_le_bytes([body[0], body[1]]);
    let voice = match kind {
        1 | 4 => Voice::Pcm {
            wave: first,
            archive: u16::from_le_bytes([body[2], body[3]]),
        },
        2 => Voice::Square {
            duty: first as u8 & 7,
        },
        3 => Voice::Noise,
        _ => return Ok(None),
    };
    Ok(Some(NoteDef {
        voice,
        base_note: body[4],
        attack: body[5],
        decay: body[6],
        sustain: body[7],
        release: body[8],
        pan: body[9],
    }))
}

#[cfg(any(test, feature = "fixtures"))]
pub fn note_bytes(def: &NoteDef) -> (u8, Vec<u8>) {
    let (kind, first, second) = match def.voice {
        Voice::Pcm { wave, archive } => (1, wave, archive),
        Voice::Square { duty } => (2, u16::from(duty), 0),
        Voice::Noise => (3, 0, 0),
    };
    let mut out = first.to_le_bytes().to_vec();
    out.extend_from_slice(&second.to_le_bytes());
    out.extend_from_slice(&[
        def.base_note,
        def.attack,
        def.decay,
        def.sustain,
        def.release,
        def.pan,
    ]);
    (kind, out)
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build(instruments: &[Option<Instrument>]) -> Vec<u8> {
    let table = COUNT + 4 + instruments.len() * 4;
    let mut records = Vec::new();
    let mut body = Vec::new();
    let region = |def: &Option<NoteDef>| match def {
        Some(def) => {
            let (kind, bytes) = note_bytes(def);
            [vec![kind, 0], bytes].concat()
        }
        None => vec![0; REGION_BYTES],
    };
    for instrument in instruments {
        let at = (table + body.len()) as u16;
        let kind = match instrument {
            None => 0,
            Some(Instrument::Single(def)) => {
                let (kind, bytes) = note_bytes(def);
                body.extend_from_slice(&bytes);
                kind
            }
            Some(Instrument::Drums { low, keys }) => {
                body.push(*low);
                body.push(low + keys.len() as u8 - 1);
                keys.iter().for_each(|k| body.extend_from_slice(&region(k)));
                DRUM_SET
            }
            Some(Instrument::Split { tops, regions }) => {
                let mut split = tops.clone();
                split.resize(SPLITS, 0);
                body.extend_from_slice(&split);
                regions
                    .iter()
                    .for_each(|r| body.extend_from_slice(&region(r)));
                KEY_SPLIT
            }
        };
        records.push(kind);
        records.extend_from_slice(&at.to_le_bytes());
        records.push(0);
    }
    let mut out = super::sound_header(b"SBNK", table - 0x10 + body.len());
    out.extend_from_slice(b"DATA");
    out.extend_from_slice(&((table - 0x10 + body.len()) as u32).to_le_bytes());
    out.resize(COUNT, 0);
    out.extend_from_slice(&(instruments.len() as u32).to_le_bytes());
    out.extend_from_slice(&records);
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(wave: u16, base_note: u8) -> NoteDef {
        NoteDef {
            voice: Voice::Pcm { wave, archive: 1 },
            base_note,
            attack: 127,
            decay: 100,
            sustain: 90,
            release: 80,
            pan: 64,
        }
    }

    #[test]
    fn reads_single_drum_and_split_instruments() {
        let square = NoteDef {
            voice: Voice::Square { duty: 3 },
            ..pcm(0, 69)
        };
        let bytes = build(&[
            Some(Instrument::Single(pcm(4, 60))),
            None,
            Some(Instrument::Drums {
                low: 36,
                keys: vec![Some(pcm(1, 36)), None, Some(square)],
            }),
            Some(Instrument::Split {
                tops: vec![59, 127],
                regions: vec![Some(pcm(2, 48)), Some(pcm(3, 72))],
            }),
        ]);
        let bank = Sbnk::parse(&bytes).unwrap();
        assert_eq!(bank.len(), 4);
        assert_eq!(bank.instrument(0).unwrap().note(10), Some(pcm(4, 60)));
        assert!(bank.instrument(1).is_none());
        let drums = bank.instrument(2).unwrap();
        assert_eq!(drums.note(36), Some(pcm(1, 36)));
        assert_eq!(drums.note(37), None);
        assert_eq!(drums.note(38), Some(square));
        assert_eq!(drums.note(35), None);
        assert_eq!(drums.note(39), None);
        let split = bank.instrument(3).unwrap();
        assert_eq!(split.note(59).unwrap().base_note, 48);
        assert_eq!(split.note(60).unwrap().base_note, 72);
    }

    #[test]
    fn other_files_are_refused() {
        assert_eq!(
            Sbnk::parse(b"SWAR").unwrap_err(),
            RomError::NotSound("SBNK")
        );
    }
}
