use super::{RomError, slice, u16_at, u32_at};

const INFO_BYTES: usize = 12;
const SWAV_INFO: usize = 0x18;
const SWAR_COUNT: usize = 0x38;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaveFormat {
    Pcm8,
    Pcm16,
    Adpcm,
}

impl WaveFormat {
    pub fn label(self) -> &'static str {
        match self {
            WaveFormat::Pcm8 => "8-bit PCM",
            WaveFormat::Pcm16 => "16-bit PCM",
            WaveFormat::Adpcm => "ADPCM",
        }
    }

    pub fn register(self) -> u32 {
        match self {
            WaveFormat::Pcm8 => 0,
            WaveFormat::Pcm16 => 1,
            WaveFormat::Adpcm => 2,
        }
    }

    fn samples_per_word(self) -> usize {
        match self {
            WaveFormat::Pcm8 => 4,
            WaveFormat::Pcm16 => 2,
            WaveFormat::Adpcm => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wave<'a> {
    pub format: WaveFormat,
    pub looped: bool,
    pub sample_rate: u16,
    pub timer: u16,
    pub loop_start: u16,
    pub loop_length: u32,
    pub data: &'a [u8],
}

impl<'a> Wave<'a> {
    pub fn parse_info(bytes: &'a [u8]) -> Result<Self, RomError> {
        let format = match bytes.first() {
            Some(0) => WaveFormat::Pcm8,
            Some(1) => WaveFormat::Pcm16,
            Some(2) => WaveFormat::Adpcm,
            Some(_) => return Err(RomError::BadWave),
            None => return Err(RomError::Truncated(1)),
        };
        let looped = *bytes.get(1).ok_or(RomError::Truncated(2))? != 0;
        Ok(Self {
            format,
            looped,
            sample_rate: u16_at(bytes, 2)?,
            timer: u16_at(bytes, 4)?,
            loop_start: u16_at(bytes, 6)?,
            loop_length: u32_at(bytes, 8)?,
            data: slice(bytes, INFO_BYTES, bytes.len())?,
        })
    }

    pub fn parse_swav(bytes: &'a [u8]) -> Result<Self, RomError> {
        if bytes.get(..4) != Some(b"SWAV") {
            return Err(RomError::NotSound("SWAV"));
        }
        Self::parse_info(slice(bytes, SWAV_INFO, bytes.len())?)
    }

    pub fn words(&self) -> usize {
        usize::from(self.loop_start) + self.loop_length as usize
    }

    pub fn sample_count(&self) -> usize {
        let header = if self.format == WaveFormat::Adpcm {
            1
        } else {
            0
        };
        self.words().saturating_sub(header) * self.format.samples_per_word()
    }

    pub fn loop_sample(&self) -> usize {
        let header = if self.format == WaveFormat::Adpcm {
            1
        } else {
            0
        };
        usize::from(self.loop_start).saturating_sub(header) * self.format.samples_per_word()
    }
}

#[derive(Debug)]
pub struct Swar<'a> {
    bytes: &'a [u8],
    offsets: Vec<usize>,
}

impl<'a> Swar<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, RomError> {
        if bytes.get(..4) != Some(b"SWAR") {
            return Err(RomError::NotSound("SWAR"));
        }
        let count = u32_at(bytes, SWAR_COUNT)? as usize;
        let offsets = (0..count)
            .map(|i| Ok(u32_at(bytes, SWAR_COUNT + 4 + i * 4)? as usize))
            .collect::<Result<Vec<_>, RomError>>()?;
        Ok(Self { bytes, offsets })
    }

    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    pub fn wave(&self, index: usize) -> Result<Wave<'a>, RomError> {
        let &start = self.offsets.get(index).ok_or(RomError::NoSuchId(index))?;
        let info = slice(self.bytes, start, self.bytes.len())?;
        let mut wave = Wave::parse_info(info)?;
        let end = (wave.words() * 4).min(wave.data.len());
        wave.data = &wave.data[..end];
        Ok(wave)
    }
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build_info(format: WaveFormat, looped: bool, loop_start: u16, data: &[u8]) -> Vec<u8> {
    let rate: u16 = 16_000;
    let words = data.len().div_ceil(4) as u32;
    let mut out = vec![format.register() as u8, u8::from(looped)];
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&((16_756_991 / u32::from(rate)) as u16).to_le_bytes());
    out.extend_from_slice(&loop_start.to_le_bytes());
    out.extend_from_slice(&(words - u32::from(loop_start)).to_le_bytes());
    out.extend_from_slice(data);
    out.resize(INFO_BYTES + words as usize * 4, 0);
    out
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build_swav(info: &[u8]) -> Vec<u8> {
    let mut out = super::sound_header(b"SWAV", 8 + info.len());
    out.extend_from_slice(b"DATA");
    out.extend_from_slice(&((8 + info.len()) as u32).to_le_bytes());
    out.extend_from_slice(info);
    out
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build_swar(infos: &[Vec<u8>]) -> Vec<u8> {
    let table = SWAR_COUNT + 4 + infos.len() * 4;
    let body: usize = infos.iter().map(Vec::len).sum();
    let mut out = super::sound_header(b"SWAR", table - 0x10 + body);
    out.extend_from_slice(b"DATA");
    out.extend_from_slice(&((table - 0x10 + body) as u32).to_le_bytes());
    out.resize(SWAR_COUNT, 0);
    out.extend_from_slice(&(infos.len() as u32).to_le_bytes());
    let mut at = table;
    for info in infos {
        out.extend_from_slice(&(at as u32).to_le_bytes());
        at += info.len();
    }
    for info in infos {
        out.extend_from_slice(info);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swav_reads_its_info_and_data() {
        let bytes = build_swav(&build_info(
            WaveFormat::Adpcm,
            true,
            2,
            &[1, 0, 5, 0, 7, 8, 9, 10, 11, 12, 13, 14],
        ));
        let wave = Wave::parse_swav(&bytes).unwrap();
        assert_eq!(wave.format, WaveFormat::Adpcm);
        assert!(wave.looped);
        assert_eq!(wave.sample_rate, 16_000);
        assert_eq!(wave.timer, 1047);
        assert_eq!(wave.loop_start, 2);
        assert_eq!(wave.loop_length, 1);
        assert_eq!(wave.sample_count(), 16);
        assert_eq!(wave.loop_sample(), 8);
        assert_eq!(wave.data[..4], [1, 0, 5, 0]);
    }

    #[test]
    fn a_swar_holds_waves_cut_to_their_length() {
        let first = build_info(WaveFormat::Pcm8, true, 1, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let second = build_info(WaveFormat::Pcm16, false, 0, &[9, 0, 10, 0]);
        let bytes = build_swar(&[first, second]);
        let swar = Swar::parse(&bytes).unwrap();
        assert_eq!(swar.len(), 2);
        let wave = swar.wave(0).unwrap();
        assert!(wave.looped);
        assert_eq!(wave.loop_sample(), 4);
        assert_eq!(wave.data, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(swar.wave(1).unwrap().sample_count(), 2);
        assert!(swar.wave(2).is_err());
    }

    #[test]
    fn other_files_are_refused() {
        assert_eq!(
            Wave::parse_swav(b"SWAR").unwrap_err(),
            RomError::NotSound("SWAV")
        );
        assert_eq!(
            Swar::parse(b"SWAV").unwrap_err(),
            RomError::NotSound("SWAR")
        );
    }
}
