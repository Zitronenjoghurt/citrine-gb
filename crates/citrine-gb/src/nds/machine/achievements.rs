use super::Nds;
use crate::achievement_map::{Region, read};

const MAIN_RAM: u32 = 0x0;
const MAIN_RAM_LEN: u32 = 0x40_0000;
const DTCM: u32 = 0x100_0000;
const DTCM_LEN: u32 = 0x4000;

impl Nds {
    pub fn achievement_read(&self, address: u32, buffer: &mut [u8]) -> usize {
        let buses = self.buses();
        let map = [
            Region {
                start: MAIN_RAM,
                len: MAIN_RAM_LEN,
                bytes: buses.main_ram(),
            },
            Region {
                start: MAIN_RAM + MAIN_RAM_LEN,
                len: DTCM - MAIN_RAM_LEN,
                bytes: &[],
            },
            Region {
                start: DTCM,
                len: DTCM_LEN,
                bytes: buses.tcm().dtcm(),
            },
        ];
        read(&map, address, buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::nds;
    use crate::gba::wait::Width;
    use crate::nds::cp15::TcmRegion;
    use crate::nds::machine::Nds;

    fn peek(ds: &Nds, address: u32) -> u8 {
        let mut byte = [0xAA];
        assert_eq!(ds.achievement_read(address, &mut byte), 1, "{address:x}");
        byte[0]
    }

    #[test]
    fn main_ram_sits_at_zero() {
        let mut ds = nds(&[], &[]);
        ds.buses_mut().arm9().write(0x0200_0100, Width::Byte, 0x42);
        ds.buses_mut().arm9().write(0x023F_FFFF, Width::Byte, 0x43);
        assert_eq!(peek(&ds, 0x100), 0x42);
        assert_eq!(peek(&ds, 0x3F_FFFF), 0x43);
    }

    #[test]
    fn the_dsi_gap_reads_zero() {
        let ds = nds(&[], &[]);
        assert_eq!(peek(&ds, 0x40_0000), 0);
        assert_eq!(peek(&ds, 0xFF_FFFF), 0);
    }

    #[test]
    fn dtcm_sits_at_16m_wherever_the_cpu_maps_it() {
        let mut ds = nds(&[], &[]);
        ds.buses_mut().tcm_mut().place_dtcm(TcmRegion {
            base: 0x0B00_0000,
            virtual_size: 0x4000,
            enabled: true,
            load_mode: false,
        });
        let tcm = ds.buses_mut().tcm_mut();
        assert!(tcm.write(0x0B00_0010, Width::Byte, 0x77));
        assert!(tcm.write(0x0B00_3FFF, Width::Byte, 0x78));
        assert_eq!(peek(&ds, 0x100_0010), 0x77);
        assert_eq!(peek(&ds, 0x100_3FFF), 0x78);
    }

    #[test]
    fn the_map_ends_after_dtcm() {
        let ds = nds(&[], &[]);
        let mut buffer = [0xAA; 4];
        assert_eq!(ds.achievement_read(0x100_3FFE, &mut buffer), 2);
        assert_eq!(ds.achievement_read(0x100_4000, &mut buffer), 0);
    }
}
