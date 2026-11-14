use super::{RomError, u32_at};

const MAX_GROWTH: usize = 16 << 20;

pub(super) fn decode(packed: &[u8]) -> Result<Vec<u8>, RomError> {
    let len = packed.len();
    let footer = u32_at(packed, len.checked_sub(8).ok_or(RomError::Truncated(8))?)?;
    let grow = u32_at(packed, len - 4)? as usize;
    let header = (footer >> 24) as usize;
    let coded = (footer & 0x00FF_FFFF) as usize;
    if header < 8 || header > coded || coded > len || grow > MAX_GROWTH {
        return Err(RomError::BadCompression(len));
    }
    let mut out = vec![0u8; len + grow];
    out[..len].copy_from_slice(packed);
    let stop = len - coded;
    let mut src = len - header;
    let mut dst = out.len();
    while src > stop {
        src -= 1;
        let mut flags = packed[src];
        for _ in 0..8 {
            if src <= stop {
                break;
            }
            if flags & 0x80 == 0 {
                src -= 1;
                dst = dst.checked_sub(1).ok_or(RomError::BadCompression(src))?;
                out[dst] = packed[src];
            } else {
                if src < stop + 2 {
                    return Err(RomError::BadCompression(src));
                }
                src -= 2;
                let pair = usize::from(packed[src]) | usize::from(packed[src + 1]) << 8;
                let run = (pair >> 12) + 3;
                let back = (pair & 0x0FFF) + 3;
                for _ in 0..run {
                    dst = dst.checked_sub(1).ok_or(RomError::BadCompression(src))?;
                    out[dst] = *out.get(dst + back).ok_or(RomError::BadCompression(src))?;
                }
            }
            flags <<= 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) fn sample(head: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut packed = head.to_vec();
    packed.extend_from_slice(&[0x00, 0xF0, 1, 2, 3, 0x10]);
    while !packed.len().is_multiple_of(4) {
        packed.push(0xFF);
    }
    let header = 8 + packed.len() - head.len() - 6;
    let coded = packed.len() + 8 - head.len();
    packed.extend_from_slice(&((header as u32) << 24 | coded as u32).to_le_bytes());
    let plain_len = head.len() + 21;
    packed.extend_from_slice(&((plain_len - packed.len() - 4) as u32).to_le_bytes());
    let mut plain = head.to_vec();
    for _ in 0..7 {
        plain.extend_from_slice(&[1, 2, 3]);
    }
    (packed, plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_with_a_kept_head_unpacks() {
        let (packed, plain) = sample(&[5; 12]);
        assert_eq!(decode(&packed).unwrap(), plain);
    }

    #[test]
    fn a_huge_size_is_refused() {
        let (mut packed, _) = sample(&[5; 12]);
        let at = packed.len() - 4;
        packed[at..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&packed).is_err());
    }

    #[test]
    fn a_back_reference_repeats_earlier_bytes() {
        let mut packed = vec![
            0xAA, 0x11, 0x22, 0x33, 0x00, 0xF0, 1, 2, 3, 0x10, 0xFF, 0xFF,
        ];
        packed.extend_from_slice(&(10u32 << 24 | 16).to_le_bytes());
        packed.extend_from_slice(&5u32.to_le_bytes());
        let mut want = vec![0xAA, 0x11, 0x22, 0x33];
        for _ in 0..7 {
            want.extend_from_slice(&[1, 2, 3]);
        }
        assert_eq!(decode(&packed).unwrap(), want);
    }
}
