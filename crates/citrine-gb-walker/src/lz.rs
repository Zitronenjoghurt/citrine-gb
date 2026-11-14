pub(crate) fn unpack(data: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size);
    let mut at = 0;
    while out.len() < size {
        let flags = *data.get(at)?;
        at += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags & (1 << bit) == 0 {
                out.push(*data.get(at)?);
                at += 1;
                continue;
            }
            let high = *data.get(at)? as usize;
            let low = *data.get(at + 1)? as usize;
            at += 2;
            let length = (high >> 4) + 3;
            let distance = (((high & 0x0F) << 8) | low) + 1;
            if distance > out.len() {
                return None;
            }
            for _ in 0..length {
                if out.len() >= size {
                    break;
                }
                out.push(out[out.len() - distance]);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
pub(crate) fn pack(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let flag_at = out.len();
        out.push(0);
        for bit in (0..8).rev() {
            if at >= data.len() {
                break;
            }
            let mut best = (0, 0);
            for distance in 1..=at.min(0x1000) {
                let mut length = 0;
                while length < 18
                    && at + length < data.len()
                    && data[at + length] == data[at + length - distance]
                {
                    length += 1;
                }
                if length > best.0 {
                    best = (length, distance);
                }
            }
            if best.0 >= 3 {
                let (length, distance) = best;
                out[flag_at] |= 1 << bit;
                out.push((((length - 3) << 4) | ((distance - 1) >> 8)) as u8);
                out.push((distance - 1) as u8);
                at += length;
            } else {
                out.push(data[at]);
                at += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_unpack_as_they_are() {
        assert_eq!(
            unpack(&[0x00, 1, 2, 3, 4, 5, 6, 7, 8], 8),
            Some(vec![1, 2, 3, 4, 5, 6, 7, 8])
        );
    }

    #[test]
    fn a_back_reference_repeats_earlier_bytes() {
        assert_eq!(unpack(&[0x40, 7, 0x20, 0x00], 6), Some(vec![7; 6]));
    }

    #[test]
    fn a_reference_before_the_start_fails() {
        assert_eq!(unpack(&[0x80, 0x00, 0x00], 3), None);
    }

    #[test]
    fn a_short_stream_fails() {
        assert_eq!(unpack(&[0x00, 1, 2], 8), None);
    }

    #[test]
    fn packed_blocks_unpack_to_the_same_bytes() {
        let block: Vec<u8> = (0..128u32).map(|i| (i / 9 * 7) as u8).collect();
        let packed = pack(&block);
        assert!(packed.len() < block.len());
        assert_eq!(unpack(&packed, 128), Some(block));
    }
}
