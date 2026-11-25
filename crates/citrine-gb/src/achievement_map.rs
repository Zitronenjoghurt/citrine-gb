pub(crate) struct Region<'a> {
    pub(crate) start: u32,
    pub(crate) len: u32,
    pub(crate) bytes: &'a [u8],
}

pub(crate) fn read(regions: &[Region<'_>], address: u32, buffer: &mut [u8]) -> usize {
    let mut done = 0;
    while done < buffer.len() {
        let Some(at) = address.checked_add(done as u32) else {
            break;
        };
        let Some(region) = regions
            .iter()
            .find(|region| at >= region.start && at - region.start < region.len)
        else {
            break;
        };
        let offset = (at - region.start) as usize;
        let room = (region.len as usize - offset).min(buffer.len() - done);
        let out = &mut buffer[done..done + room];
        let have = region.bytes.get(offset..).unwrap_or_default();
        let copied = have.len().min(room);
        out[..copied].copy_from_slice(&have[..copied]);
        out[copied..].fill(0);
        done += room;
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOW: [u8; 4] = [1, 2, 3, 4];
    const HIGH: [u8; 2] = [9, 8];

    fn map() -> [Region<'static>; 2] {
        [
            Region {
                start: 0,
                len: 4,
                bytes: &LOW,
            },
            Region {
                start: 4,
                len: 4,
                bytes: &HIGH,
            },
        ]
    }

    #[test]
    fn a_read_crosses_from_one_region_into_the_next() {
        let mut buffer = [0xAA; 4];
        assert_eq!(read(&map(), 2, &mut buffer), 4);
        assert_eq!(buffer, [3, 4, 9, 8]);
    }

    #[test]
    fn a_region_longer_than_its_bytes_reads_zero_past_them() {
        let mut buffer = [0xAA; 3];
        assert_eq!(read(&map(), 5, &mut buffer), 3);
        assert_eq!(buffer, [8, 0, 0]);
    }

    #[test]
    fn a_read_stops_at_the_end_of_the_map() {
        let mut buffer = [0xAA; 4];
        assert_eq!(read(&map(), 6, &mut buffer), 2);
        assert_eq!(read(&map(), 8, &mut buffer), 0);
        assert_eq!(read(&map(), u32::MAX, &mut buffer), 0);
    }
}
