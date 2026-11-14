const END_MARK: &[u8] = b"|-DESMUME SAVE-|";
const START_MARK: &[u8] =
    b"|<--Snip above here to create a raw sav by excluding this DeSmuME savedata footer:";

pub fn strip(data: &[u8]) -> &[u8] {
    if !data.ends_with(END_MARK) {
        return data;
    }
    data.windows(START_MARK.len())
        .rposition(|window| window == START_MARK)
        .map_or(data, |at| &data[..at])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsv(raw: &[u8]) -> Vec<u8> {
        let mut file = raw.to_vec();
        file.extend_from_slice(START_MARK);
        file.extend_from_slice(&[0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x00]);
        file.extend_from_slice(&[0; 16]);
        file.extend_from_slice(END_MARK);
        file
    }

    #[test]
    fn a_dsv_loses_its_footer() {
        let raw = vec![0x42; 0x8_0000];
        assert_eq!(strip(&dsv(&raw)), &raw[..]);
    }

    #[test]
    fn a_raw_save_stays_whole() {
        let raw = vec![0x42; 0x200];
        assert_eq!(strip(&raw), &raw[..]);
    }

    #[test]
    fn an_end_mark_without_a_start_keeps_the_file() {
        let mut file = vec![1, 2, 3];
        file.extend_from_slice(END_MARK);
        assert_eq!(strip(&file), &file[..]);
    }
}
