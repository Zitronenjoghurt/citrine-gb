use super::{RomError, slice, u32_at};

const DATA_OFFSET: usize = 0x18;

pub fn sequence_data(bytes: &[u8]) -> Result<&[u8], RomError> {
    if bytes.get(..4) != Some(b"SSEQ") {
        return Err(RomError::NotSound("SSEQ"));
    }
    let start = u32_at(bytes, DATA_OFFSET)? as usize;
    slice(bytes, start, bytes.len())
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build(data: &[u8]) -> Vec<u8> {
    let mut out = super::sound_header(b"SSEQ", 12 + data.len());
    out.extend_from_slice(b"DATA");
    out.extend_from_slice(&((12 + data.len()) as u32).to_le_bytes());
    out.extend_from_slice(&0x1Cu32.to_le_bytes());
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_starts_where_the_header_says() {
        let bytes = build(&[0xFE, 0x01, 0x00, 0xFF]);
        assert_eq!(sequence_data(&bytes).unwrap(), [0xFE, 0x01, 0x00, 0xFF]);
        assert_eq!(
            sequence_data(b"SBNK").unwrap_err(),
            RomError::NotSound("SSEQ")
        );
    }
}
