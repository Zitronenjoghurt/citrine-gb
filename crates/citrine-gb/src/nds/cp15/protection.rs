use super::Fetch;

const REGIONS: usize = 8;
const REGION_MASK: u32 = 0xFFFF_F03F;
const REGION_ENABLE: u32 = 1;
const REGION_BASE: u32 = 0xFFFF_F000;
const REGION_BITS: u32 = 0xFF;
const DATA: usize = 0;
const CODE: usize = 1;
const PAGE_BITS: u32 = 12;

const DIRECT_BOOT_CACHEABLE: u32 = 0x42;
const DIRECT_BOOT_BUFFERABLE: u32 = 0x02;
const DIRECT_BOOT_DATA_PERMISSIONS: u32 = 0x1511_1011;
const DIRECT_BOOT_CODE_PERMISSIONS: u32 = 0x0510_0011;
const DIRECT_BOOT_REGIONS: [u32; REGIONS] = [
    0x0400_0033,
    0x0200_002B,
    0x0000_0000,
    0x0800_0035,
    0x0300_001B,
    0x0000_0000,
    0xFFFF_001D,
    0x027F_F017,
];

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Protection {
    cacheable: [u32; 2],
    bufferable: u32,
    permissions: [u32; 2],
    regions: [u32; REGIONS],
}

impl Protection {
    pub fn read(&self, crn: u32, crm: u32, opcode2: u32) -> u32 {
        match (crn, crm, opcode2) {
            (2, 0, side @ 0..=1) => self.cacheable[side as usize],
            (3, 0, 0) => self.bufferable,
            (5, 0, side @ 0..=1) => legacy(self.permissions[side as usize]),
            (5, 0, side @ 2..=3) => self.permissions[side as usize - 2],
            (6, region @ 0..=7, 0..=1) => self.regions[region as usize],
            _ => 0,
        }
    }

    pub fn write(&mut self, crn: u32, crm: u32, opcode2: u32, value: u32) {
        match (crn, crm, opcode2) {
            (2, 0, side @ 0..=1) => self.cacheable[side as usize] = value & REGION_BITS,
            (3, 0, 0) => self.bufferable = value & REGION_BITS,
            (5, 0, side @ 0..=1) => self.permissions[side as usize] = extended(value),
            (5, 0, side @ 2..=3) => self.permissions[side as usize - 2] = value,
            (6, region @ 0..=7, 0..=1) => self.regions[region as usize] = value & REGION_MASK,
            _ => {}
        }
    }

    pub fn allows(&self, addr: u32, fetch: Fetch, write: bool, privileged: bool) -> bool {
        let Some(region) = (0..REGIONS).rev().find(|&i| covers(self.regions[i], addr)) else {
            return false;
        };
        let side = match fetch {
            Fetch::Code => CODE,
            Fetch::Data => DATA,
        };
        let permission = (self.permissions[side] >> (4 * region)) & 0xF;
        let (read, written) = match (permission, privileged) {
            (1..=3, true) | (3, false) => (true, true),
            (2, false) | (5, true) | (6, _) => (true, false),
            _ => (false, false),
        };
        if write { written } else { read }
    }

    pub fn cacheable(&self, addr: u32, fetch: Fetch) -> bool {
        let side = match fetch {
            Fetch::Code => CODE,
            Fetch::Data => DATA,
        };
        (0..REGIONS)
            .rev()
            .find(|&i| covers(self.regions[i], addr))
            .is_some_and(|region| self.cacheable[side] >> region & 1 != 0)
    }

    pub fn cached_pages(&self) -> impl Iterator<Item = (u32, u32, [bool; 2])> + '_ {
        (0..REGIONS)
            .filter(|&i| self.regions[i] & REGION_ENABLE != 0)
            .map(|i| {
                let region = self.regions[i];
                let size = 2u64 << ((region >> 1) & 0x1F);
                let mask = !(size - 1) as u32 & REGION_BASE;
                let pages = (size >> PAGE_BITS).max(1) as u32;
                let sides = [CODE, DATA].map(|side| self.cacheable[side] >> i & 1 != 0);
                ((region & mask) >> PAGE_BITS, pages, sides)
            })
    }

    pub fn direct_boot(&mut self) {
        self.cacheable = [DIRECT_BOOT_CACHEABLE; 2];
        self.bufferable = DIRECT_BOOT_BUFFERABLE;
        self.permissions = [DIRECT_BOOT_DATA_PERMISSIONS, DIRECT_BOOT_CODE_PERMISSIONS];
        self.regions = DIRECT_BOOT_REGIONS;
    }
}

fn covers(region: u32, addr: u32) -> bool {
    let size = 2u64 << ((region >> 1) & 0x1F);
    let mask = !(size - 1) as u32 & REGION_BASE;
    region & REGION_ENABLE != 0 && (addr ^ region) & mask == 0
}

fn legacy(permissions: u32) -> u32 {
    (0..REGIONS as u32).fold(0, |value, region| {
        value | (((permissions >> (4 * region)) & 3) << (2 * region))
    })
}

fn extended(value: u32) -> u32 {
    (0..REGIONS as u32).fold(0, |permissions, region| {
        permissions | (((value >> (2 * region)) & 3) << (4 * region))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERYONE_READ_WRITE: u32 = 3;

    fn region(base: u32, size_field: u32) -> u32 {
        base | (size_field << 1) | REGION_ENABLE
    }

    fn open(regions: &[(usize, u32)], permission: u32) -> Protection {
        let mut protection = Protection::default();
        for &(index, value) in regions {
            protection.write(6, index as u32, 0, value);
        }
        let all = (0..8).fold(0, |all, i| all | (permission << (4 * i)));
        protection.write(5, 0, 2, all);
        protection.write(5, 0, 3, all);
        protection
    }

    fn data_read(protection: &Protection, addr: u32) -> bool {
        protection.allows(addr, Fetch::Data, false, true)
    }

    #[test]
    fn cacheable_bits_keep_one_byte_per_side() {
        let mut protection = Protection::default();
        protection.write(2, 0, 0, 0xFFFF_FF42);
        protection.write(2, 0, 1, 0x0000_0181);
        assert_eq!(protection.read(2, 0, 0), 0x42);
        assert_eq!(protection.read(2, 0, 1), 0x81);
    }

    #[test]
    fn bufferable_bits_have_only_a_data_side() {
        let mut protection = Protection::default();
        protection.write(3, 0, 0, 0x1_0002);
        protection.write(3, 0, 1, 0xFF);
        assert_eq!(protection.read(3, 0, 0), 0x02);
        assert_eq!(protection.read(3, 0, 1), 0);
    }

    #[test]
    fn extended_permissions_read_back_whole() {
        let mut protection = Protection::default();
        protection.write(5, 0, 2, 0x1511_1011);
        protection.write(5, 0, 3, 0x0510_0011);
        assert_eq!(protection.read(5, 0, 2), 0x1511_1011);
        assert_eq!(protection.read(5, 0, 3), 0x0510_0011);
    }

    #[test]
    fn legacy_permissions_read_the_low_two_bits_of_each_region() {
        let mut protection = Protection::default();
        protection.write(5, 0, 2, 0x1511_1063);
        assert_eq!(protection.read(5, 0, 0), 0b01_01_01_01_01_00_10_11);
    }

    #[test]
    fn legacy_permission_writes_clear_the_extended_high_bits() {
        let mut protection = Protection::default();
        protection.write(5, 0, 3, 0xFFFF_FFFF);
        protection.write(5, 0, 1, 0xFFFF_0000 | 0b11_10_01_00);
        assert_eq!(protection.read(5, 0, 3), 0x0000_3210);
        assert_eq!(protection.read(5, 0, 1), 0b11_10_01_00);
    }

    #[test]
    fn region_registers_clear_their_reserved_bits() {
        let mut protection = Protection::default();
        protection.write(6, 3, 0, 0x0800_0FFF);
        assert_eq!(protection.read(6, 3, 0), 0x0800_003F);
    }

    #[test]
    fn region_registers_are_shared_by_both_opcode2_values() {
        let mut protection = Protection::default();
        protection.write(6, 5, 1, 0x0100_001D);
        assert_eq!(protection.read(6, 5, 0), 0x0100_001D);
    }

    #[test]
    fn an_address_in_no_region_is_refused() {
        let protection = open(&[(1, region(0x0200_0000, 21))], EVERYONE_READ_WRITE);
        assert!(data_read(&protection, 0x0200_0000));
        assert!(data_read(&protection, 0x023F_FFFF));
        assert!(!data_read(&protection, 0x0240_0000));
        assert!(!data_read(&protection, 0x01FF_FFFC));
    }

    #[test]
    fn a_disabled_region_matches_nothing() {
        let protection = open(&[(1, 0x0200_002A)], EVERYONE_READ_WRITE);
        assert!(!data_read(&protection, 0x0200_0000));
    }

    #[test]
    fn a_4gb_region_covers_every_address() {
        let protection = open(&[(0, region(0, 31))], EVERYONE_READ_WRITE);
        assert!(data_read(&protection, 0));
        assert!(data_read(&protection, 0xFFFF_FFFC));
    }

    #[test]
    fn sizes_below_4kb_still_cover_4kb() {
        let protection = open(&[(0, region(0x0300_0000, 5))], EVERYONE_READ_WRITE);
        assert!(data_read(&protection, 0x0300_0FFF));
        assert!(!data_read(&protection, 0x0300_1000));
    }

    #[test]
    fn the_higher_region_wins_where_regions_overlap() {
        let mut protection = Protection::default();
        protection.write(6, 0, 0, region(0, 31));
        protection.write(6, 4, 0, region(0x0300_0000, 13));
        protection.write(5, 0, 2, 0x0000_0003);
        assert!(data_read(&protection, 0x0200_0000));
        assert!(!data_read(&protection, 0x0300_0000));
        protection.write(5, 0, 2, 0x0003_0000);
        assert!(!data_read(&protection, 0x0200_0000));
        assert!(data_read(&protection, 0x0300_3FFC));
    }

    #[test]
    fn the_lower_region_shows_where_the_higher_one_ends() {
        let mut protection = open(&[(2, region(0x0200_0000, 21))], 0);
        protection.write(6, 6, 0, region(0x0200_0000, 11));
        protection.write(5, 0, 2, 0x0300_0300);
        assert!(data_read(&protection, 0x0200_0FFC));
        assert!(data_read(&protection, 0x0200_1000));
        protection.write(5, 0, 2, 0x0300_0000);
        assert!(data_read(&protection, 0x0200_0FFC));
        assert!(!data_read(&protection, 0x0200_1000));
    }

    #[test]
    fn code_fetches_use_the_instruction_permissions() {
        let mut protection = Protection::default();
        protection.write(6, 0, 0, region(0, 31));
        protection.write(5, 0, 2, 3);
        assert!(protection.allows(0x100, Fetch::Data, false, true));
        assert!(!protection.allows(0x100, Fetch::Code, false, true));
        protection.write(5, 0, 3, 6);
        assert!(protection.allows(0x100, Fetch::Code, false, false));
    }

    fn rights(permission: u32) -> [bool; 4] {
        let protection = open(&[(0, region(0, 31))], permission);
        [
            protection.allows(0, Fetch::Data, false, true),
            protection.allows(0, Fetch::Data, true, true),
            protection.allows(0, Fetch::Data, false, false),
            protection.allows(0, Fetch::Data, true, false),
        ]
    }

    #[test]
    fn permission_0_allows_nothing() {
        assert_eq!(rights(0), [false; 4]);
    }

    #[test]
    fn permission_1_is_privileged_read_write_only() {
        assert_eq!(rights(1), [true, true, false, false]);
    }

    #[test]
    fn permission_2_lets_user_mode_read() {
        assert_eq!(rights(2), [true, true, true, false]);
    }

    #[test]
    fn permission_3_lets_everyone_read_and_write() {
        assert_eq!(rights(3), [true; 4]);
    }

    #[test]
    fn permission_5_is_privileged_read_only() {
        assert_eq!(rights(5), [true, false, false, false]);
    }

    #[test]
    fn permission_6_is_read_only_for_everyone() {
        assert_eq!(rights(6), [true, false, true, false]);
    }

    #[test]
    fn reserved_permissions_allow_nothing() {
        for permission in [4, 7, 8, 11, 15] {
            assert_eq!(rights(permission), [false; 4], "permission {permission}");
        }
    }

    #[test]
    fn direct_boot_opens_main_ram_and_the_bios() {
        let mut protection = Protection::default();
        protection.direct_boot();
        assert!(protection.allows(0x0200_0000, Fetch::Code, false, true));
        assert!(protection.allows(0x0200_0000, Fetch::Data, true, true));
        assert!(protection.allows(0xFFFF_0000, Fetch::Code, false, true));
        assert!(!protection.allows(0xFFFF_0000, Fetch::Data, true, true));
        assert!(protection.allows(0x0300_0000, Fetch::Data, true, true));
        assert!(!protection.allows(0x0300_0000, Fetch::Code, false, true));
        assert!(!protection.allows(0x0100_0000, Fetch::Data, false, true));
    }
}
