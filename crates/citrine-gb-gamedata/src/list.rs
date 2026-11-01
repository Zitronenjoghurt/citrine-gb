const END: u8 = 0xFF;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Shape {
    pub capacity: usize,
    pub mon_size: usize,
    pub name_len: usize,
}

pub(crate) struct Entry<'a> {
    pub listed: u8,
    pub mon: &'a [u8],
    pub ot_name: &'a [u8],
    pub nickname: &'a [u8],
}

impl Shape {
    pub fn size(self) -> usize {
        2 + self.capacity * (1 + self.mon_size + 2 * self.name_len)
    }

    pub fn entries(self, bytes: &[u8]) -> Option<Vec<Entry<'_>>> {
        let bytes = bytes.get(..self.size())?;
        let count = usize::from(bytes[0]);
        if count > self.capacity || bytes[1 + count] != END {
            return None;
        }
        let mons = 2 + self.capacity;
        let ot_names = mons + self.capacity * self.mon_size;
        let nicknames = ot_names + self.capacity * self.name_len;
        Some(
            (0..count)
                .map(|i| Entry {
                    listed: bytes[1 + i],
                    mon: &bytes[mons + i * self.mon_size..][..self.mon_size],
                    ot_name: &bytes[ot_names + i * self.name_len..][..self.name_len],
                    nickname: &bytes[nicknames + i * self.name_len..][..self.name_len],
                })
                .collect(),
        )
    }
}

pub(crate) fn items(bytes: &[u8], capacity: usize) -> Option<Vec<crate::ItemStack>> {
    let count = usize::from(*bytes.first()?);
    if count > capacity || *bytes.get(1 + 2 * count)? != END {
        return None;
    }
    Some(
        bytes[1..1 + 2 * count]
            .chunks(2)
            .map(|pair| crate::ItemStack {
                item: pair[0],
                count: pair[1],
            })
            .collect(),
    )
}

pub(crate) fn key_items(bytes: &[u8], capacity: usize) -> Option<Vec<crate::ItemStack>> {
    let count = usize::from(*bytes.first()?);
    if count > capacity || *bytes.get(1 + count)? != END {
        return None;
    }
    Some(
        bytes[1..1 + count]
            .iter()
            .map(|&item| crate::ItemStack { item, count: 1 })
            .collect(),
    )
}

pub(crate) fn flags(bytes: &[u8], count: u16) -> Vec<u16> {
    (1..=count)
        .filter(|n| {
            let bit = usize::from(n - 1);
            bytes.get(bit / 8).is_some_and(|b| b >> (bit % 8) & 1 == 1)
        })
        .collect()
}

pub(crate) fn bcd(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |total, &b| {
        total * 100 + u32::from(b >> 4) * 10 + u32::from(b & 0x0F)
    })
}

pub(crate) fn be(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |total, &b| total << 8 | u32::from(b))
}
