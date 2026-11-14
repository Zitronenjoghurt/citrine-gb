use super::Cpu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoName {
    pub name: &'static str,
    pub addr: u32,
    pub size: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoGroup {
    pub name: &'static str,
    pub rows: &'static [IoName],
}

const fn io(name: &'static str, addr: u32, size: u8) -> IoName {
    IoName { name, addr, size }
}

pub fn groups(cpu: Cpu) -> &'static [IoGroup] {
    match cpu {
        Cpu::Arm9 => ARM9,
        Cpu::Arm7 => ARM7,
    }
}

pub fn name(cpu: Cpu, addr: u32) -> Option<&'static str> {
    groups(cpu)
        .iter()
        .flat_map(|group| group.rows)
        .find(|row| row.addr == addr)
        .map(|row| row.name)
}

const ARM9: &[IoGroup] = &[
    IoGroup {
        name: "Display A",
        rows: &[
            io("DISPCNT", 0x04000000, 4),
            io("DISPSTAT", 0x04000004, 2),
            io("VCOUNT", 0x04000006, 2),
            io("BG0CNT", 0x04000008, 2),
            io("BG1CNT", 0x0400000A, 2),
            io("BG2CNT", 0x0400000C, 2),
            io("BG3CNT", 0x0400000E, 2),
            io("BG0HOFS", 0x04000010, 2),
            io("BG0VOFS", 0x04000012, 2),
            io("BG1HOFS", 0x04000014, 2),
            io("BG1VOFS", 0x04000016, 2),
            io("BG2HOFS", 0x04000018, 2),
            io("BG2VOFS", 0x0400001A, 2),
            io("BG3HOFS", 0x0400001C, 2),
            io("BG3VOFS", 0x0400001E, 2),
            io("BG2PA", 0x04000020, 2),
            io("BG2PB", 0x04000022, 2),
            io("BG2PC", 0x04000024, 2),
            io("BG2PD", 0x04000026, 2),
            io("BG2X", 0x04000028, 4),
            io("BG2Y", 0x0400002C, 4),
            io("BG3PA", 0x04000030, 2),
            io("BG3PB", 0x04000032, 2),
            io("BG3PC", 0x04000034, 2),
            io("BG3PD", 0x04000036, 2),
            io("BG3X", 0x04000038, 4),
            io("BG3Y", 0x0400003C, 4),
            io("WIN0H", 0x04000040, 2),
            io("WIN1H", 0x04000042, 2),
            io("WIN0V", 0x04000044, 2),
            io("WIN1V", 0x04000046, 2),
            io("WININ", 0x04000048, 2),
            io("WINOUT", 0x0400004A, 2),
            io("MOSAIC", 0x0400004C, 2),
            io("BLDCNT", 0x04000050, 2),
            io("BLDALPHA", 0x04000052, 2),
            io("BLDY", 0x04000054, 2),
            io("DISP3DCNT", 0x04000060, 2),
            io("DISPCAPCNT", 0x04000064, 4),
            io("DISP_MMEM_FIFO", 0x04000068, 4),
            io("MASTER_BRIGHT", 0x0400006C, 2),
        ],
    },
    IoGroup {
        name: "Display B",
        rows: &[
            io("DB_DISPCNT", 0x04001000, 4),
            io("DB_BG0CNT", 0x04001008, 2),
            io("DB_BG1CNT", 0x0400100A, 2),
            io("DB_BG2CNT", 0x0400100C, 2),
            io("DB_BG3CNT", 0x0400100E, 2),
            io("DB_BG0HOFS", 0x04001010, 2),
            io("DB_BG0VOFS", 0x04001012, 2),
            io("DB_BG1HOFS", 0x04001014, 2),
            io("DB_BG1VOFS", 0x04001016, 2),
            io("DB_BG2HOFS", 0x04001018, 2),
            io("DB_BG2VOFS", 0x0400101A, 2),
            io("DB_BG3HOFS", 0x0400101C, 2),
            io("DB_BG3VOFS", 0x0400101E, 2),
            io("DB_BG2PA", 0x04001020, 2),
            io("DB_BG2PB", 0x04001022, 2),
            io("DB_BG2PC", 0x04001024, 2),
            io("DB_BG2PD", 0x04001026, 2),
            io("DB_BG2X", 0x04001028, 4),
            io("DB_BG2Y", 0x0400102C, 4),
            io("DB_BG3PA", 0x04001030, 2),
            io("DB_BG3PB", 0x04001032, 2),
            io("DB_BG3PC", 0x04001034, 2),
            io("DB_BG3PD", 0x04001036, 2),
            io("DB_BG3X", 0x04001038, 4),
            io("DB_BG3Y", 0x0400103C, 4),
            io("DB_WIN0H", 0x04001040, 2),
            io("DB_WIN1H", 0x04001042, 2),
            io("DB_WIN0V", 0x04001044, 2),
            io("DB_WIN1V", 0x04001046, 2),
            io("DB_WININ", 0x04001048, 2),
            io("DB_WINOUT", 0x0400104A, 2),
            io("DB_MOSAIC", 0x0400104C, 2),
            io("DB_BLDCNT", 0x04001050, 2),
            io("DB_BLDALPHA", 0x04001052, 2),
            io("DB_BLDY", 0x04001054, 2),
            io("DB_MASTER_BRIGHT", 0x0400106C, 2),
        ],
    },
    IoGroup {
        name: "3D",
        rows: &[
            io("RDLINES_COUNT", 0x04000320, 1),
            io("EDGE_COLOR", 0x04000330, 16),
            io("ALPHA_TEST_REF", 0x04000340, 1),
            io("CLEAR_COLOR", 0x04000350, 4),
            io("CLEAR_DEPTH", 0x04000354, 2),
            io("CLRIMAGE_OFFSET", 0x04000356, 2),
            io("FOG_COLOR", 0x04000358, 4),
            io("FOG_OFFSET", 0x0400035C, 2),
            io("FOG_TABLE", 0x04000360, 32),
            io("TOON_TABLE", 0x04000380, 64),
            io("GXFIFO", 0x04000400, 64),
            io("MTX_MODE", 0x04000440, 4),
            io("MTX_PUSH", 0x04000444, 4),
            io("MTX_POP", 0x04000448, 4),
            io("MTX_STORE", 0x0400044C, 4),
            io("MTX_RESTORE", 0x04000450, 4),
            io("MTX_IDENTITY", 0x04000454, 4),
            io("MTX_LOAD_4x4", 0x04000458, 4),
            io("MTX_LOAD_4x3", 0x0400045C, 4),
            io("MTX_MULT_4x4", 0x04000460, 4),
            io("MTX_MULT_4x3", 0x04000464, 4),
            io("MTX_MULT_3x3", 0x04000468, 4),
            io("MTX_SCALE", 0x0400046C, 4),
            io("MTX_TRANS", 0x04000470, 4),
            io("COLOR", 0x04000480, 4),
            io("NORMAL", 0x04000484, 4),
            io("TEXCOORD", 0x04000488, 4),
            io("VTX_16", 0x0400048C, 4),
            io("VTX_10", 0x04000490, 4),
            io("VTX_XY", 0x04000494, 4),
            io("VTX_XZ", 0x04000498, 4),
            io("VTX_YZ", 0x0400049C, 4),
            io("VTX_DIFF", 0x040004A0, 4),
            io("POLYGON_ATTR", 0x040004A4, 4),
            io("TEXIMAGE_PARAM", 0x040004A8, 4),
            io("PLTT_BASE", 0x040004AC, 4),
            io("DIF_AMB", 0x040004C0, 4),
            io("SPE_EMI", 0x040004C4, 4),
            io("LIGHT_VECTOR", 0x040004C8, 4),
            io("LIGHT_COLOR", 0x040004CC, 4),
            io("SHININESS", 0x040004D0, 4),
            io("BEGIN_VTXS", 0x04000500, 4),
            io("END_VTXS", 0x04000504, 4),
            io("SWAP_BUFFERS", 0x04000540, 4),
            io("VIEWPORT", 0x04000580, 4),
            io("BOX_TEST", 0x040005C0, 4),
            io("POS_TEST", 0x040005C4, 4),
            io("VEC_TEST", 0x040005C8, 4),
            io("GXSTAT", 0x04000600, 4),
            io("RAM_COUNT", 0x04000604, 4),
            io("DISP_1DOT_DEPTH", 0x04000610, 2),
            io("POS_RESULT", 0x04000620, 16),
            io("VEC_RESULT", 0x04000630, 6),
            io("CLIPMTX_RESULT", 0x04000640, 64),
            io("VECMTX_RESULT", 0x04000680, 36),
        ],
    },
    IoGroup {
        name: "DMA",
        rows: &[
            io("DMA0SAD", 0x040000B0, 4),
            io("DMA0DAD", 0x040000B4, 4),
            io("DMA0CNT", 0x040000B8, 4),
            io("DMA1SAD", 0x040000BC, 4),
            io("DMA1DAD", 0x040000C0, 4),
            io("DMA1CNT", 0x040000C4, 4),
            io("DMA2SAD", 0x040000C8, 4),
            io("DMA2DAD", 0x040000CC, 4),
            io("DMA2CNT", 0x040000D0, 4),
            io("DMA3SAD", 0x040000D4, 4),
            io("DMA3DAD", 0x040000D8, 4),
            io("DMA3CNT", 0x040000DC, 4),
            io("DMA0FILL", 0x040000E0, 4),
            io("DMA1FILL", 0x040000E4, 4),
            io("DMA2FILL", 0x040000E8, 4),
            io("DMA3FILL", 0x040000EC, 4),
        ],
    },
    IoGroup {
        name: "Timers",
        rows: &[
            io("TM0CNT_L", 0x04000100, 2),
            io("TM0CNT_H", 0x04000102, 2),
            io("TM1CNT_L", 0x04000104, 2),
            io("TM1CNT_H", 0x04000106, 2),
            io("TM2CNT_L", 0x04000108, 2),
            io("TM2CNT_H", 0x0400010A, 2),
            io("TM3CNT_L", 0x0400010C, 2),
            io("TM3CNT_H", 0x0400010E, 2),
        ],
    },
    IoGroup {
        name: "Keypad",
        rows: &[io("KEYINPUT", 0x04000130, 2), io("KEYCNT", 0x04000132, 2)],
    },
    IoGroup {
        name: "IPC",
        rows: &[
            io("IPCSYNC", 0x04000180, 2),
            io("IPCFIFOCNT", 0x04000184, 2),
            io("IPCFIFOSEND", 0x04000188, 4),
            io("IPCFIFORECV", 0x04100000, 4),
        ],
    },
    IoGroup {
        name: "Card",
        rows: &[
            io("AUXSPICNT", 0x040001A0, 2),
            io("AUXSPIDATA", 0x040001A2, 2),
            io("ROMCTRL", 0x040001A4, 4),
        ],
    },
    IoGroup {
        name: "Memory",
        rows: &[
            io("EXMEMCNT", 0x04000204, 2),
            io("VRAMCNT_A", 0x04000240, 1),
            io("VRAMCNT_B", 0x04000241, 1),
            io("VRAMCNT_C", 0x04000242, 1),
            io("VRAMCNT_D", 0x04000243, 1),
            io("VRAMCNT_E", 0x04000244, 1),
            io("VRAMCNT_F", 0x04000245, 1),
            io("VRAMCNT_G", 0x04000246, 1),
            io("WRAMCNT", 0x04000247, 1),
            io("VRAMCNT_H", 0x04000248, 1),
            io("VRAMCNT_I", 0x04000249, 1),
        ],
    },
    IoGroup {
        name: "Interrupts",
        rows: &[
            io("IME", 0x04000208, 2),
            io("IE", 0x04000210, 4),
            io("IF", 0x04000214, 4),
        ],
    },
    IoGroup {
        name: "Math",
        rows: &[
            io("DIVCNT", 0x04000280, 2),
            io("DIV_NUMER", 0x04000290, 8),
            io("DIV_DENOM", 0x04000298, 8),
            io("DIV_RESULT", 0x040002A0, 8),
            io("DIVREM_RESULT", 0x040002A8, 8),
            io("SQRTCNT", 0x040002B0, 2),
            io("SQRT_RESULT", 0x040002B4, 4),
            io("SQRT_PARAM", 0x040002B8, 8),
        ],
    },
    IoGroup {
        name: "Power",
        rows: &[io("POSTFLG", 0x04000300, 4), io("POWCNT1", 0x04000304, 2)],
    },
];

const ARM7: &[IoGroup] = &[
    IoGroup {
        name: "Display",
        rows: &[io("DISPSTAT", 0x04000004, 2), io("VCOUNT", 0x04000006, 2)],
    },
    IoGroup {
        name: "DMA",
        rows: &[
            io("DMA0SAD", 0x040000B0, 4),
            io("DMA0DAD", 0x040000B4, 4),
            io("DMA0CNT", 0x040000B8, 4),
            io("DMA1SAD", 0x040000BC, 4),
            io("DMA1DAD", 0x040000C0, 4),
            io("DMA1CNT", 0x040000C4, 4),
            io("DMA2SAD", 0x040000C8, 4),
            io("DMA2DAD", 0x040000CC, 4),
            io("DMA2CNT", 0x040000D0, 4),
            io("DMA3SAD", 0x040000D4, 4),
            io("DMA3DAD", 0x040000D8, 4),
            io("DMA3CNT", 0x040000DC, 4),
        ],
    },
    IoGroup {
        name: "Timers",
        rows: &[
            io("TM0CNT_L", 0x04000100, 2),
            io("TM0CNT_H", 0x04000102, 2),
            io("TM1CNT_L", 0x04000104, 2),
            io("TM1CNT_H", 0x04000106, 2),
            io("TM2CNT_L", 0x04000108, 2),
            io("TM2CNT_H", 0x0400010A, 2),
            io("TM3CNT_L", 0x0400010C, 2),
            io("TM3CNT_H", 0x0400010E, 2),
        ],
    },
    IoGroup {
        name: "Keypad",
        rows: &[
            io("KEYINPUT", 0x04000130, 2),
            io("KEYCNT", 0x04000132, 2),
            io("EXTKEYIN", 0x04000136, 2),
        ],
    },
    IoGroup {
        name: "Serial",
        rows: &[
            io("SIODATA32", 0x04000120, 4),
            io("SIOCNT", 0x04000128, 4),
            io("RCNT", 0x04000134, 2),
            io("RTC", 0x04000138, 1),
        ],
    },
    IoGroup {
        name: "IPC",
        rows: &[
            io("IPCSYNC", 0x04000180, 2),
            io("IPCFIFOCNT", 0x04000184, 2),
            io("IPCFIFOSEND", 0x04000188, 4),
            io("IPCFIFORECV", 0x04100000, 4),
        ],
    },
    IoGroup {
        name: "Card",
        rows: &[
            io("AUXSPICNT", 0x040001A0, 2),
            io("AUXSPIDATA", 0x040001A2, 2),
            io("ROMCTRL", 0x040001A4, 4),
        ],
    },
    IoGroup {
        name: "SPI",
        rows: &[io("SPICNT", 0x040001C0, 2), io("SPIDATA", 0x040001C2, 2)],
    },
    IoGroup {
        name: "Memory",
        rows: &[
            io("EXMEMSTAT", 0x04000204, 2),
            io("WIFIWAITCNT", 0x04000206, 2),
            io("VRAMSTAT", 0x04000240, 1),
            io("WRAMSTAT", 0x04000241, 1),
            io("BIOSPROT", 0x04000308, 4),
        ],
    },
    IoGroup {
        name: "Interrupts",
        rows: &[
            io("IME", 0x04000208, 4),
            io("IE", 0x04000210, 4),
            io("IF", 0x04000214, 4),
        ],
    },
    IoGroup {
        name: "Power",
        rows: &[
            io("POSTFLG", 0x04000300, 1),
            io("HALTCNT", 0x04000301, 1),
            io("POWCNT2", 0x04000304, 2),
        ],
    },
    IoGroup {
        name: "Sound",
        rows: &[
            io("SOUND0CNT", 0x04000400, 4),
            io("SOUND0SAD", 0x04000404, 4),
            io("SOUND0TMR", 0x04000408, 2),
            io("SOUND0PNT", 0x0400040A, 2),
            io("SOUND0LEN", 0x0400040C, 4),
            io("SOUND1CNT", 0x04000410, 4),
            io("SOUND1SAD", 0x04000414, 4),
            io("SOUND1TMR", 0x04000418, 2),
            io("SOUND1PNT", 0x0400041A, 2),
            io("SOUND1LEN", 0x0400041C, 4),
            io("SOUND2CNT", 0x04000420, 4),
            io("SOUND2SAD", 0x04000424, 4),
            io("SOUND2TMR", 0x04000428, 2),
            io("SOUND2PNT", 0x0400042A, 2),
            io("SOUND2LEN", 0x0400042C, 4),
            io("SOUND3CNT", 0x04000430, 4),
            io("SOUND3SAD", 0x04000434, 4),
            io("SOUND3TMR", 0x04000438, 2),
            io("SOUND3PNT", 0x0400043A, 2),
            io("SOUND3LEN", 0x0400043C, 4),
            io("SOUND4CNT", 0x04000440, 4),
            io("SOUND4SAD", 0x04000444, 4),
            io("SOUND4TMR", 0x04000448, 2),
            io("SOUND4PNT", 0x0400044A, 2),
            io("SOUND4LEN", 0x0400044C, 4),
            io("SOUND5CNT", 0x04000450, 4),
            io("SOUND5SAD", 0x04000454, 4),
            io("SOUND5TMR", 0x04000458, 2),
            io("SOUND5PNT", 0x0400045A, 2),
            io("SOUND5LEN", 0x0400045C, 4),
            io("SOUND6CNT", 0x04000460, 4),
            io("SOUND6SAD", 0x04000464, 4),
            io("SOUND6TMR", 0x04000468, 2),
            io("SOUND6PNT", 0x0400046A, 2),
            io("SOUND6LEN", 0x0400046C, 4),
            io("SOUND7CNT", 0x04000470, 4),
            io("SOUND7SAD", 0x04000474, 4),
            io("SOUND7TMR", 0x04000478, 2),
            io("SOUND7PNT", 0x0400047A, 2),
            io("SOUND7LEN", 0x0400047C, 4),
            io("SOUND8CNT", 0x04000480, 4),
            io("SOUND8SAD", 0x04000484, 4),
            io("SOUND8TMR", 0x04000488, 2),
            io("SOUND8PNT", 0x0400048A, 2),
            io("SOUND8LEN", 0x0400048C, 4),
            io("SOUND9CNT", 0x04000490, 4),
            io("SOUND9SAD", 0x04000494, 4),
            io("SOUND9TMR", 0x04000498, 2),
            io("SOUND9PNT", 0x0400049A, 2),
            io("SOUND9LEN", 0x0400049C, 4),
            io("SOUND10CNT", 0x040004A0, 4),
            io("SOUND10SAD", 0x040004A4, 4),
            io("SOUND10TMR", 0x040004A8, 2),
            io("SOUND10PNT", 0x040004AA, 2),
            io("SOUND10LEN", 0x040004AC, 4),
            io("SOUND11CNT", 0x040004B0, 4),
            io("SOUND11SAD", 0x040004B4, 4),
            io("SOUND11TMR", 0x040004B8, 2),
            io("SOUND11PNT", 0x040004BA, 2),
            io("SOUND11LEN", 0x040004BC, 4),
            io("SOUND12CNT", 0x040004C0, 4),
            io("SOUND12SAD", 0x040004C4, 4),
            io("SOUND12TMR", 0x040004C8, 2),
            io("SOUND12PNT", 0x040004CA, 2),
            io("SOUND12LEN", 0x040004CC, 4),
            io("SOUND13CNT", 0x040004D0, 4),
            io("SOUND13SAD", 0x040004D4, 4),
            io("SOUND13TMR", 0x040004D8, 2),
            io("SOUND13PNT", 0x040004DA, 2),
            io("SOUND13LEN", 0x040004DC, 4),
            io("SOUND14CNT", 0x040004E0, 4),
            io("SOUND14SAD", 0x040004E4, 4),
            io("SOUND14TMR", 0x040004E8, 2),
            io("SOUND14PNT", 0x040004EA, 2),
            io("SOUND14LEN", 0x040004EC, 4),
            io("SOUND15CNT", 0x040004F0, 4),
            io("SOUND15SAD", 0x040004F4, 4),
            io("SOUND15TMR", 0x040004F8, 2),
            io("SOUND15PNT", 0x040004FA, 2),
            io("SOUND15LEN", 0x040004FC, 4),
            io("SOUNDCNT", 0x04000500, 2),
            io("SOUNDBIAS", 0x04000504, 2),
            io("SNDCAP0CNT", 0x04000508, 1),
            io("SNDCAP1CNT", 0x04000509, 1),
            io("SNDCAP0DAD", 0x04000510, 4),
            io("SNDCAP0LEN", 0x04000514, 2),
            io("SNDCAP1DAD", 0x04000518, 4),
            io("SNDCAP1LEN", 0x0400051C, 2),
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::{Cpu, groups, name};

    #[test]
    fn every_address_is_unique_and_no_two_registers_overlap() {
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let mut rows: Vec<_> = groups(cpu).iter().flat_map(|group| group.rows).collect();
            rows.sort_by_key(|row| row.addr);
            for pair in rows.windows(2) {
                assert!(
                    pair[0].addr + u32::from(pair[0].size) <= pair[1].addr,
                    "{cpu:?}: {} overlaps {}",
                    pair[0].name,
                    pair[1].name
                );
            }
        }
    }

    #[test]
    fn known_registers_sit_at_their_gbatek_addresses() {
        let cases = [
            (Cpu::Arm9, 0x0400_0000, "DISPCNT"),
            (Cpu::Arm9, 0x0400_1000, "DB_DISPCNT"),
            (Cpu::Arm9, 0x0400_0180, "IPCSYNC"),
            (Cpu::Arm7, 0x0400_0180, "IPCSYNC"),
            (Cpu::Arm9, 0x0400_0184, "IPCFIFOCNT"),
            (Cpu::Arm7, 0x0400_0184, "IPCFIFOCNT"),
            (Cpu::Arm9, 0x0400_0240, "VRAMCNT_A"),
            (Cpu::Arm9, 0x0400_0248, "VRAMCNT_H"),
            (Cpu::Arm9, 0x0400_0304, "POWCNT1"),
            (Cpu::Arm7, 0x0400_0304, "POWCNT2"),
            (Cpu::Arm7, 0x0400_01C0, "SPICNT"),
            (Cpu::Arm9, 0x0400_01A0, "AUXSPICNT"),
            (Cpu::Arm7, 0x0400_01A0, "AUXSPICNT"),
            (Cpu::Arm9, 0x0400_0204, "EXMEMCNT"),
            (Cpu::Arm7, 0x0400_0204, "EXMEMSTAT"),
            (Cpu::Arm9, 0x0400_0600, "GXSTAT"),
            (Cpu::Arm7, 0x0400_04F0, "SOUND15CNT"),
            (Cpu::Arm9, 0x0410_0000, "IPCFIFORECV"),
        ];
        for (cpu, addr, expected) in cases {
            assert_eq!(name(cpu, addr), Some(expected), "{cpu:?} {addr:08X}");
        }
    }

    #[test]
    fn each_cpu_only_names_its_own_registers() {
        assert_eq!(name(Cpu::Arm7, 0x0400_0000), None);
        assert_eq!(name(Cpu::Arm7, 0x0400_0304), Some("POWCNT2"));
        assert_eq!(name(Cpu::Arm9, 0x0400_01C0), None);
        assert_eq!(name(Cpu::Arm9, 0x0400_051C), None);
    }
}
