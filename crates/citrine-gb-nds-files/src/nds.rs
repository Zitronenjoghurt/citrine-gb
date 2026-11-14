use super::{FileRange, RomError, blz, file_ranges, name_table, slice, u32_at};

const NITRO_CODE: u32 = 0xDEC0_0621;

#[derive(Debug)]
pub struct NdsRom<'a> {
    bytes: &'a [u8],
    files: Vec<FileRange>,
    paths: Vec<(String, usize)>,
}

impl<'a> NdsRom<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, RomError> {
        let fnt_at = u32_at(bytes, 0x40)? as usize;
        let fnt_len = u32_at(bytes, 0x44)? as usize;
        let fat_at = u32_at(bytes, 0x48)? as usize;
        let fat_len = u32_at(bytes, 0x4C)? as usize;
        let fnt = slice(bytes, fnt_at, fnt_at + fnt_len)?;
        let fat = slice(bytes, fat_at, fat_at + fat_len)?;
        Ok(Self {
            bytes,
            files: file_ranges(fat, 0)?,
            paths: name_table(fnt)?,
        })
    }

    pub fn game_code(&self) -> [u8; 4] {
        let mut code = [0; 4];
        code.copy_from_slice(&self.bytes[0x0C..0x10]);
        code
    }

    pub fn file(&self, path: &str) -> Result<&'a [u8], RomError> {
        let path = path.trim_start_matches('/');
        let id = self
            .paths
            .iter()
            .find(|(p, _)| p == path)
            .map(|&(_, id)| id)
            .ok_or_else(|| RomError::NoSuchFile(path.to_owned()))?;
        self.file_by_id(id)
    }

    pub fn file_by_id(&self, id: usize) -> Result<&'a [u8], RomError> {
        let range = self.files.get(id).ok_or(RomError::NoSuchId(id))?;
        slice(self.bytes, range.start, range.end)
    }

    pub fn arm9(&self) -> Result<Vec<u8>, RomError> {
        let at = u32_at(self.bytes, 0x20)? as usize;
        let ram = u32_at(self.bytes, 0x28)? as usize;
        let len = u32_at(self.bytes, 0x2C)? as usize;
        let code = slice(self.bytes, at, at + len)?;
        if u32_at(self.bytes, at + len)? != NITRO_CODE {
            return Err(RomError::NoModuleParams);
        }
        let params = u32_at(self.bytes, at + len + 4)? as usize;
        let packed_end = u32_at(code, params + 0x14)? as usize;
        if packed_end == 0 {
            return Ok(code.to_vec());
        }
        let split = packed_end
            .checked_sub(ram)
            .filter(|&split| split <= len)
            .ok_or(RomError::BadCompression(packed_end))?;
        let mut out = blz::decode(&code[..split])?;
        out.extend_from_slice(&code[split..]);
        Ok(out)
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.paths.iter().map(|(p, _)| p.as_str())
    }

    pub fn named_files(&self) -> impl Iterator<Item = (&str, usize)> {
        self.paths.iter().map(|(p, id)| (p.as_str(), *id))
    }
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build(code: &[u8; 4], files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    build_with_arm9(code, &[], files)
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build_with_arm9(code: &[u8; 4], arm9: &[u8], files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    struct Dir {
        name: String,
        files: Vec<(String, Vec<u8>)>,
        dirs: Vec<usize>,
    }
    let mut dirs = vec![Dir {
        name: String::new(),
        files: Vec::new(),
        dirs: Vec::new(),
    }];
    for (path, data) in files {
        let mut parts: Vec<&str> = path.split('/').collect();
        let file = parts.pop().unwrap_or_default();
        let mut dir = 0;
        for part in parts {
            dir = match dirs[dir]
                .dirs
                .iter()
                .copied()
                .find(|&d| dirs[d].name == part)
            {
                Some(d) => d,
                None => {
                    dirs.push(Dir {
                        name: part.to_owned(),
                        files: Vec::new(),
                        dirs: Vec::new(),
                    });
                    let id = dirs.len() - 1;
                    dirs[dir].dirs.push(id);
                    id
                }
            };
        }
        dirs[dir].files.push((file.to_owned(), data.clone()));
    }
    let mut main = Vec::new();
    let mut subs = Vec::new();
    let mut blobs = Vec::new();
    let main_len = dirs.len() * 8;
    for (index, dir) in dirs.iter().enumerate() {
        main.extend_from_slice(&((main_len + subs.len()) as u32).to_le_bytes());
        main.extend_from_slice(&(blobs.len() as u16).to_le_bytes());
        let parent = if index == 0 {
            dirs.len() as u16
        } else {
            0xF000
        };
        main.extend_from_slice(&parent.to_le_bytes());
        for (name, data) in &dir.files {
            subs.push(name.len() as u8);
            subs.extend_from_slice(name.as_bytes());
            blobs.push(data.clone());
        }
        for &child in &dir.dirs {
            subs.push(0x80 | dirs[child].name.len() as u8);
            subs.extend_from_slice(dirs[child].name.as_bytes());
            subs.extend_from_slice(&(0xF000 | child as u16).to_le_bytes());
        }
        subs.push(0);
    }
    let fnt = [main, subs].concat();
    let fnt_at = 0x200;
    let fat_at = fnt_at + fnt.len();
    let data_at = fat_at + blobs.len() * 8;
    let mut rom = vec![0u8; fnt_at];
    rom[0x0C..0x10].copy_from_slice(code);
    rom[0x40..0x44].copy_from_slice(&(fnt_at as u32).to_le_bytes());
    rom[0x44..0x48].copy_from_slice(&(fnt.len() as u32).to_le_bytes());
    rom[0x48..0x4C].copy_from_slice(&(fat_at as u32).to_le_bytes());
    rom[0x4C..0x50].copy_from_slice(&((blobs.len() * 8) as u32).to_le_bytes());
    rom.extend_from_slice(&fnt);
    let mut at = data_at;
    for blob in &blobs {
        rom.extend_from_slice(&(at as u32).to_le_bytes());
        rom.extend_from_slice(&((at + blob.len()) as u32).to_le_bytes());
        at += blob.len();
    }
    for blob in &blobs {
        rom.extend_from_slice(blob);
    }
    let arm9_at = rom.len();
    rom[0x20..0x24].copy_from_slice(&(arm9_at as u32).to_le_bytes());
    rom[0x28..0x2C].copy_from_slice(&0x0200_0000u32.to_le_bytes());
    rom[0x2C..0x30].copy_from_slice(&(arm9.len() as u32).to_le_bytes());
    rom.extend_from_slice(arm9);
    rom.extend_from_slice(&NITRO_CODE.to_le_bytes());
    rom.extend_from_slice(&0u32.to_le_bytes());
    rom.extend_from_slice(&0u32.to_le_bytes());
    rom
}

#[cfg(any(test, feature = "fixtures"))]
pub fn arm9_with_params(body: &[u8], packed_end: u32) -> Vec<u8> {
    let mut arm9 = vec![0u8; 0x20];
    arm9[0x14..0x18].copy_from_slice(&packed_end.to_le_bytes());
    arm9.extend_from_slice(body);
    arm9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files_by_path_in_nested_folders() {
        let bytes = build(
            b"ADAE",
            &[
                ("arm9ovltable.bin", vec![1, 2]),
                ("poketool/personal/personal.narc", vec![3, 4, 5]),
                ("poketool/personal/wotbl.narc", vec![6]),
                ("a/0/0/2", vec![7, 8]),
            ],
        );
        let rom = NdsRom::parse(&bytes).unwrap();
        assert_eq!(rom.game_code(), *b"ADAE");
        assert_eq!(rom.file("arm9ovltable.bin").unwrap(), [1, 2]);
        assert_eq!(
            rom.file("poketool/personal/personal.narc").unwrap(),
            [3, 4, 5]
        );
        assert_eq!(rom.file("/poketool/personal/wotbl.narc").unwrap(), [6]);
        assert_eq!(rom.file("a/0/0/2").unwrap(), [7, 8]);
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let bytes = build(b"ADAE", &[("a/0/0/2", vec![7])]);
        let rom = NdsRom::parse(&bytes).unwrap();
        assert_eq!(
            rom.file("a/0/0/3").unwrap_err(),
            RomError::NoSuchFile("a/0/0/3".into())
        );
    }

    #[test]
    fn a_truncated_rom_is_an_error() {
        let bytes = build(b"ADAE", &[("a/0/0/2", vec![7; 32])]);
        assert!(NdsRom::parse(&bytes[..0x210]).is_err());
    }

    #[test]
    fn plain_arm9_code_comes_back_as_it_is() {
        let arm9 = arm9_with_params(&[9, 8, 7], 0);
        let bytes = build_with_arm9(b"CPUE", &arm9, &[]);
        assert_eq!(NdsRom::parse(&bytes).unwrap().arm9().unwrap(), arm9);
    }

    #[test]
    fn compressed_arm9_code_is_unpacked() {
        let (mut packed, mut want) = blz::sample(&arm9_with_params(&[], 0));
        let end = 0x0200_0000 + packed.len() as u32;
        packed[0x14..0x18].copy_from_slice(&end.to_le_bytes());
        want[0x14..0x18].copy_from_slice(&end.to_le_bytes());
        packed.extend_from_slice(&[0xEE, 0xEF]);
        want.extend_from_slice(&[0xEE, 0xEF]);
        let bytes = build_with_arm9(b"IPGE", &packed, &[]);
        assert_eq!(NdsRom::parse(&bytes).unwrap().arm9().unwrap(), want);
    }
}
