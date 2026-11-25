const USER_DATA_BYTES: usize = 0x40;
const PSEUDO: usize = 0x04;
const AUTHENTIC: usize = 0x10;
const PROFILE: usize = 0x1C;
const FLAG: usize = 0x20;
const CRC: usize = 0x3C;
const DIRTY: u32 = 1;
const USERID_HI_MASK: u32 = 0x7FF;

pub fn saved_user(ram: &[u8]) -> Option<u64> {
    blocks(ram)
        .map(|block| login_id(block, AUTHENTIC))
        .find(|&userid| userid != 0)
}

pub fn saved_profile(ram: &[u8], userid: u64) -> Option<u32> {
    let mut found = None;
    for block in blocks(ram) {
        if login_id(block, AUTHENTIC) != userid && login_id(block, PSEUDO) != userid {
            continue;
        }
        let profile = word(block, PROFILE);
        if profile != 0 && i32::try_from(profile).is_ok() {
            return Some(profile);
        }
        found = Some(0);
    }
    found
}

fn blocks(ram: &[u8]) -> impl Iterator<Item = &[u8]> {
    (0..ram.len().saturating_sub(USER_DATA_BYTES - 1))
        .step_by(4)
        .map(move |at| &ram[at..at + USER_DATA_BYTES])
        .filter(|block| word(block, 0) == USER_DATA_BYTES as u32 && sealed(block))
}

fn login_id(block: &[u8], at: usize) -> u64 {
    (u64::from(word(block, at) & USERID_HI_MASK) << 32) | u64::from(word(block, at + 4))
}

fn sealed(block: &[u8]) -> bool {
    let stored = word(block, CRC);
    let mut bytes = [0; CRC];
    bytes.copy_from_slice(&block[..CRC]);
    let flag = word(block, FLAG) & !DIRTY;
    bytes[FLAG..FLAG + 4].copy_from_slice(&flag.to_le_bytes());
    crc32(&bytes) == stored || crc32(&block[..CRC]) == stored
}

fn word(block: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([block[at], block[at + 1], block[at + 2], block[at + 3]])
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    const USERID: u64 = 0x0298_7654_3210;

    fn user_data(userid: u64, profile: u32, dirty: bool) -> Vec<u8> {
        let mut block = vec![0; USER_DATA_BYTES];
        block[..4].copy_from_slice(&(USER_DATA_BYTES as u32).to_le_bytes());
        for at in [PSEUDO, AUTHENTIC] {
            let hi = u32::try_from(userid >> 32).unwrap() | (1 << 11);
            block[at..at + 4].copy_from_slice(&hi.to_le_bytes());
            block[at + 4..at + 8].copy_from_slice(&(userid as u32).to_le_bytes());
            block[at + 8..at + 12].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        }
        block[PROFILE..PROFILE + 4].copy_from_slice(&profile.to_le_bytes());
        block[0x24..0x28].copy_from_slice(b"AMCE");
        let crc = crc32(&block[..CRC]);
        block[CRC..].copy_from_slice(&crc.to_le_bytes());
        if dirty {
            block[FLAG] |= DIRTY as u8;
        }
        block
    }

    fn ram_with(block: &[u8], at: usize) -> Vec<u8> {
        let mut ram = vec![0x40; 0x4000];
        ram[at..at + block.len()].copy_from_slice(block);
        ram
    }

    #[test]
    fn the_crc_is_standard_crc32() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn the_saved_profile_is_found_for_the_firmware_user() {
        let ram = ram_with(&user_data(USERID, 123_456_789, false), 0x1F04);
        assert_eq!(saved_profile(&ram, USERID), Some(123_456_789));
    }

    #[test]
    fn a_dirty_flag_keeps_the_crc_it_had_before() {
        let ram = ram_with(&user_data(USERID, 42, true), 0x800);
        assert_eq!(saved_profile(&ram, USERID), Some(42));
    }

    #[test]
    fn another_users_data_and_a_broken_crc_are_ignored() {
        let ram = ram_with(&user_data(USERID + 1, 42, false), 0x800);
        assert_eq!(saved_profile(&ram, USERID), None);
        let mut block = user_data(USERID, 42, false);
        block[PROFILE] ^= 1;
        assert_eq!(saved_profile(&ram_with(&block, 0x800), USERID), None);
    }

    #[test]
    fn the_saved_user_is_the_authentic_id_of_sealed_user_data() {
        let ram = ram_with(&user_data(USERID, 42, true), 0x2A0);
        assert_eq!(saved_user(&ram), Some(USERID));
        let mut block = user_data(USERID, 42, false);
        block[AUTHENTIC + 4] ^= 1;
        assert_eq!(saved_user(&ram_with(&block, 0x2A0)), None);
        let mut block = user_data(USERID, 0, false);
        block[AUTHENTIC..AUTHENTIC + 12].fill(0);
        let crc = crc32(&block[..CRC]);
        block[CRC..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(saved_user(&ram_with(&block, 0x2A0)), None);
    }

    #[test]
    fn user_data_without_a_profile_reads_as_zero() {
        let ram = ram_with(&user_data(USERID, 0, false), 0x800);
        assert_eq!(saved_profile(&ram, USERID), Some(0));
    }
}
