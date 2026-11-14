use super::{FileRange, RomError, file_ranges, slice, u16_at, u32_at};

#[derive(Debug)]
pub struct Narc<'a> {
    data: &'a [u8],
    files: Vec<FileRange>,
}

impl<'a> Narc<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, RomError> {
        if bytes.get(..4) != Some(b"NARC") {
            return Err(RomError::NotNarc);
        }
        let header_len = usize::from(u16_at(bytes, 0x0C)?);
        let sections = u16_at(bytes, 0x0E)?;
        let mut at = header_len;
        let mut fat = None;
        let mut data = None;
        for _ in 0..sections {
            let magic = slice(bytes, at, at + 4)?;
            let len = u32_at(bytes, at + 4)? as usize;
            let body = slice(bytes, at + 8, at + len.max(8))?;
            match magic {
                b"BTAF" => fat = Some(body),
                b"GMIF" => data = Some(body),
                _ => {}
            }
            at += len.max(8);
        }
        let fat = fat.ok_or(RomError::MissingSection("BTAF"))?;
        let data = data.ok_or(RomError::MissingSection("GMIF"))?;
        let count = usize::from(u16_at(fat, 0)?);
        let table = slice(fat, 4, 4 + count * 8)?;
        Ok(Self {
            data,
            files: file_ranges(table, 0)?,
        })
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn file(&self, id: usize) -> Result<&'a [u8], RomError> {
        let range = self.files.get(id).ok_or(RomError::NoSuchId(id))?;
        slice(self.data, range.start, range.end)
    }

    pub fn files(&self) -> impl Iterator<Item = Result<&'a [u8], RomError>> + '_ {
        (0..self.files.len()).map(|id| self.file(id))
    }
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build(files: &[Vec<u8>]) -> Vec<u8> {
    let mut fat = Vec::new();
    fat.extend_from_slice(&(files.len() as u16).to_le_bytes());
    fat.extend_from_slice(&[0, 0]);
    let mut data = Vec::new();
    for file in files {
        fat.extend_from_slice(&(data.len() as u32).to_le_bytes());
        fat.extend_from_slice(&((data.len() + file.len()) as u32).to_le_bytes());
        data.extend_from_slice(file);
        while data.len() % 4 != 0 {
            data.push(0xFF);
        }
    }
    let fnt = [4u32.to_le_bytes().as_slice(), &[0, 0, 1, 0], &[0, 0, 0, 0]].concat();
    let section = |magic: &[u8; 4], body: &[u8]| {
        [
            magic.as_slice(),
            &((body.len() + 8) as u32).to_le_bytes(),
            body,
        ]
        .concat()
    };
    let body = [
        section(b"BTAF", &fat),
        section(b"BTNF", &fnt),
        section(b"GMIF", &data),
    ]
    .concat();
    let mut narc = b"NARC".to_vec();
    narc.extend_from_slice(&[0xFE, 0xFF, 0x00, 0x01]);
    narc.extend_from_slice(&((body.len() + 16) as u32).to_le_bytes());
    narc.extend_from_slice(&16u16.to_le_bytes());
    narc.extend_from_slice(&3u16.to_le_bytes());
    narc.extend_from_slice(&body);
    narc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_file_in_order() {
        let bytes = build(&[vec![1, 2, 3], vec![], vec![4; 44]]);
        let narc = Narc::parse(&bytes).unwrap();
        assert_eq!(narc.len(), 3);
        assert_eq!(narc.file(0).unwrap(), [1, 2, 3]);
        assert!(narc.file(1).unwrap().is_empty());
        assert_eq!(narc.file(2).unwrap(), [4; 44]);
        assert_eq!(narc.file(3).unwrap_err(), RomError::NoSuchId(3));
    }

    #[test]
    fn rejects_other_files() {
        assert_eq!(Narc::parse(b"RGCN\0\0\0\0").unwrap_err(), RomError::NotNarc);
    }

    #[test]
    fn a_file_past_the_data_is_an_error() {
        let mut bytes = build(&[vec![1, 2, 3]]);
        let end_at = 0x10 + 8 + 4 + 4;
        bytes[end_at..end_at + 4].copy_from_slice(&999u32.to_le_bytes());
        let narc = Narc::parse(&bytes).unwrap();
        assert!(narc.file(0).is_err());
    }
}
