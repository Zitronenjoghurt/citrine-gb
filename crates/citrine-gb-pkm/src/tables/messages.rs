use super::{ExtractError, u16_at, u32_at};
use crate::pkm::{Gen, text};

pub fn gen4_messages(file: &[u8]) -> Result<Vec<String>, ExtractError> {
    let short = |at: usize| ExtractError::BadMessages(at);
    let count = usize::from(u16_at(file.get(..4).ok_or(short(0))?, 0));
    let seed = u32::from(u16_at(file, 2));
    (0..count)
        .map(|i| {
            let key = seed.wrapping_mul(0x2FD).wrapping_mul(i as u32 + 1) & 0xFFFF;
            let key = key | key << 16;
            let at = 4 + i * 8;
            let entry = file.get(at..at + 8).ok_or(short(at))?;
            let offset = (u32_at(entry, 0) ^ key) as usize;
            let len = (u32_at(entry, 4) ^ key) as usize;
            let bytes = file.get(offset..offset + len * 2).ok_or(short(offset))?;
            let mut key = 0x91BD3u32.wrapping_mul(i as u32 + 1) as u16;
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|w| {
                    let unit = u16::from_le_bytes([w[0], w[1]]) ^ key;
                    key = key.wrapping_add(0x493D);
                    unit
                })
                .collect();
            Ok(text::decode(Gen::Four, &units))
        })
        .collect()
}

pub fn gen5_messages(file: &[u8]) -> Result<Vec<String>, ExtractError> {
    let short = |at: usize| ExtractError::BadMessages(at);
    let header = file.get(..16).ok_or(short(0))?;
    let count = usize::from(u16_at(header, 2));
    let section = u32_at(header, 12) as usize;
    (0..count)
        .map(|i| {
            let at = section + 4 + i * 8;
            let entry = file.get(at..at + 8).ok_or(short(at))?;
            let offset = section + u32_at(entry, 0) as usize;
            let len = usize::from(u16_at(entry, 4));
            let bytes = file.get(offset..offset + len * 2).ok_or(short(offset))?;
            let mut units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|w| u16::from_le_bytes([w[0], w[1]]))
                .collect();
            let mut key = units.last().map_or(0, |&last| last ^ 0xFFFF);
            for unit in units.iter_mut().rev() {
                *unit ^= key;
                key = key.rotate_right(3);
            }
            Ok(text::decode(Gen::Five, &units))
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn gen4_file(lines: &[&str], seed: u16) -> Vec<u8> {
    let encoded: Vec<Vec<u16>> = lines
        .iter()
        .map(|line| {
            let mut units: Vec<u16> = line
                .chars()
                .map(|c| {
                    let one: [u16; 2] =
                        text::encode(Gen::Four, crate::Language::English, &c.to_string()).unwrap();
                    one[0]
                })
                .collect();
            units.push(0xFFFF);
            units
        })
        .collect();
    let mut file = Vec::new();
    file.extend_from_slice(&(lines.len() as u16).to_le_bytes());
    file.extend_from_slice(&seed.to_le_bytes());
    let mut offset = 4 + lines.len() * 8;
    for (i, units) in encoded.iter().enumerate() {
        let key = u32::from(seed)
            .wrapping_mul(0x2FD)
            .wrapping_mul(i as u32 + 1)
            & 0xFFFF;
        let key = key | key << 16;
        file.extend_from_slice(&(offset as u32 ^ key).to_le_bytes());
        file.extend_from_slice(&(units.len() as u32 ^ key).to_le_bytes());
        offset += units.len() * 2;
    }
    for (i, units) in encoded.iter().enumerate() {
        let mut key = 0x91BD3u32.wrapping_mul(i as u32 + 1) as u16;
        for &unit in units {
            file.extend_from_slice(&(unit ^ key).to_le_bytes());
            key = key.wrapping_add(0x493D);
        }
    }
    file
}

#[cfg(test)]
pub(crate) fn gen5_file(lines: &[&str]) -> Vec<u8> {
    let section = 16;
    let table_len = 4 + lines.len() * 8;
    let mut entries = Vec::new();
    let mut texts = Vec::new();
    for line in lines {
        let mut units: Vec<u16> = line.encode_utf16().collect();
        units.push(0xFFFF);
        let mut key = 0x7C89u16.wrapping_add(entries.len() as u16);
        let mut encrypted = units.clone();
        for unit in encrypted.iter_mut().rev() {
            *unit ^= key;
            key = key.rotate_right(3);
        }
        entries.extend_from_slice(&((table_len + texts.len()) as u32).to_le_bytes());
        entries.extend_from_slice(&(units.len() as u16).to_le_bytes());
        entries.extend_from_slice(&[0, 0]);
        for unit in encrypted {
            texts.extend_from_slice(&unit.to_le_bytes());
        }
    }
    let section_len = table_len + texts.len();
    let mut file = Vec::new();
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&(lines.len() as u16).to_le_bytes());
    file.extend_from_slice(&(section_len as u32).to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&(section as u32).to_le_bytes());
    file.extend_from_slice(&(section_len as u32).to_le_bytes());
    file.extend_from_slice(&entries);
    file.extend_from_slice(&texts);
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_gen4_message_files() {
        let file = gen4_file(&["-----", "Bulbasaur", "Turtwig"], 0x1234);
        assert_eq!(
            gen4_messages(&file).unwrap(),
            ["-----", "Bulbasaur", "Turtwig"]
        );
    }

    #[test]
    fn decrypts_gen4_japanese_and_korean_names() {
        let file = gen4_file(&["ナエトル", "모부기"], 0xBEEF);
        assert_eq!(gen4_messages(&file).unwrap(), ["ナエトル", "모부기"]);
    }

    #[test]
    fn decrypts_gen5_message_files() {
        let file = gen5_file(&["Snivy", "ツタージャ", "주리비얀"]);
        assert_eq!(
            gen5_messages(&file).unwrap(),
            ["Snivy", "ツタージャ", "주리비얀"]
        );
    }

    #[test]
    fn decrypts_long_files_whose_keys_wrap() {
        let names: Vec<String> = (0..400).map(|i| format!("Name {i}")).collect();
        let lines: Vec<&str> = names.iter().map(String::as_str).collect();
        let file = gen4_file(&lines, 0xFFFF);
        assert_eq!(gen4_messages(&file).unwrap(), names);
    }

    #[test]
    fn a_cut_file_is_an_error() {
        let file = gen4_file(&["Turtwig"], 1);
        assert!(gen4_messages(&file[..file.len() - 4]).is_err());
        let file = gen5_file(&["Snivy"]);
        assert!(gen5_messages(&file[..20]).is_err());
    }
}
