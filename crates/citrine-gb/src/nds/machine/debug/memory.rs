use crate::debug::code::{Address, BlockId, NDS_BANK_SHIFT};
use crate::debug::memory::MemoryBlock;
use crate::gba::wait::Width;
use crate::nds::Cpu;
use crate::nds::bus::Buses;
use crate::nds::io_names;
use crate::nds::tcm::{DTCM_SIZE, ITCM_SIZE};
use crate::nds::vram::{Bank, Purpose};

const IO_BASE: u32 = 0x0400_0000;
const FIFO_BASE: u32 = 0x0410_0000;
const FIFO_SIZE: u32 = 0x20;
const ARM9_BIOS_BASE: u32 = 0xFFFF_0000;
const ARM7_WRAM_BASE: u32 = 0x0380_0000;
const PALETTE_OAM_SIZE: u32 = 0x800;
const ROW: u32 = 0x10;

const VRAM_NAMES: [&str; 9] = [
    "VRAM A", "VRAM B", "VRAM C", "VRAM D", "VRAM E", "VRAM F", "VRAM G", "VRAM H", "VRAM I",
];

fn vram_base(cpu: Cpu, purpose: Purpose) -> Option<u32> {
    match (cpu, purpose) {
        (Cpu::Arm9, Purpose::BgA) => Some(0x0600_0000),
        (Cpu::Arm9, Purpose::BgB) => Some(0x0620_0000),
        (Cpu::Arm9, Purpose::ObjA) => Some(0x0640_0000),
        (Cpu::Arm9, Purpose::ObjB) => Some(0x0660_0000),
        (Cpu::Arm9, Purpose::Lcdc) => Some(0x0680_0000),
        (Cpu::Arm7, Purpose::Arm7) => Some(0x0600_0000),
        _ => None,
    }
}

fn arm9_vram_purpose(addr: u32) -> Purpose {
    match addr >> 21 & 7 {
        0 => Purpose::BgA,
        1 => Purpose::BgB,
        2 => Purpose::ObjA,
        3 => Purpose::ObjB,
        _ => Purpose::Lcdc,
    }
}

fn io_size(cpu: Cpu) -> u32 {
    let end = io_names::groups(cpu)
        .iter()
        .flat_map(|group| group.rows)
        .filter(|row| row.addr < FIFO_BASE)
        .map(|row| row.addr + u32::from(row.size))
        .max()
        .unwrap_or(IO_BASE);
    (end - IO_BASE).next_multiple_of(ROW)
}

fn bank_of(base: u32) -> Option<u16> {
    u16::try_from(base >> NDS_BANK_SHIFT).ok()
}

fn block(id: BlockId, name: &'static str, bank: u16, size: usize) -> MemoryBlock {
    let start = Address::at(id, bank, 0).nds_address().unwrap_or(0);
    let writable = !matches!(
        id,
        BlockId::NdsBios9 | BlockId::NdsBios7 | BlockId::NdsIo | BlockId::NdsFifo
    );
    MemoryBlock {
        id,
        name,
        start,
        size: size as u32,
        first_bank: bank,
        banks: 1,
        writable,
    }
}

pub(super) fn blocks(buses: &Buses, cpu: Cpu, out: &mut Vec<MemoryBlock>) {
    out.clear();
    let tcm = buses.tcm();
    let shared = buses.shared_wram(cpu).len();
    match cpu {
        Cpu::Arm9 => {
            if tcm.itcm_region().enabled {
                out.push(block(BlockId::NdsItcm, "ITCM", 0, ITCM_SIZE));
            }
            let dtcm = tcm.dtcm_region();
            if dtcm.enabled
                && let Some(bank) = bank_of(dtcm.base)
            {
                out.push(block(BlockId::NdsDtcm, "DTCM", bank, DTCM_SIZE));
            }
            out.push(block(
                BlockId::NdsPalette,
                "Palette",
                0,
                PALETTE_OAM_SIZE as usize,
            ));
            out.push(block(BlockId::NdsOam, "OAM", 0, PALETTE_OAM_SIZE as usize));
            out.push(block(
                BlockId::NdsBios9,
                "BIOS",
                0,
                buses.bios(Cpu::Arm9).len(),
            ));
        }
        Cpu::Arm7 => {
            out.push(block(
                BlockId::NdsBios7,
                "BIOS",
                0,
                buses.bios(Cpu::Arm7).len(),
            ));
            out.push(block(
                BlockId::NdsArm7Wram,
                "ARM7 WRAM",
                0,
                buses.arm7_wram().len(),
            ));
        }
    }
    out.push(block(
        BlockId::NdsMainRam,
        "Main RAM",
        0,
        buses.main_ram().len(),
    ));
    if shared > 0 {
        out.push(block(BlockId::NdsSharedWram, "Shared WRAM", 0, shared));
    }
    out.push(block(BlockId::NdsIo, "I/O", 0, io_size(cpu) as usize));
    out.push(block(
        BlockId::NdsFifo,
        "IPC FIFO and card data",
        0,
        FIFO_SIZE as usize,
    ));
    for (index, bank) in Bank::ALL.into_iter().enumerate() {
        let Some((purpose, offset)) = buses.vram().mapping(bank) else {
            continue;
        };
        let Some(base) = vram_base(cpu, purpose) else {
            continue;
        };
        if let Some(first) = bank_of(base + offset as u32) {
            out.push(block(
                BlockId::NdsVram(index as u8),
                VRAM_NAMES[index],
                first,
                bank.size(),
            ));
        }
    }
    out.sort_by_key(|block| block.start);
}

pub(super) fn locate(buses: &Buses, cpu: Cpu, addr: u32) -> Option<Address> {
    let at = |id, offset| Some(Address::at(id, 0, offset));
    if cpu == Cpu::Arm9 {
        let tcm = buses.tcm();
        let itcm = tcm.itcm_region();
        if itcm.enabled && u64::from(addr) < itcm.virtual_size {
            return at(BlockId::NdsItcm, addr % ITCM_SIZE as u32);
        }
        let dtcm = tcm.dtcm_region();
        if dtcm.enabled
            && addr >= dtcm.base
            && u64::from(addr - dtcm.base) < dtcm.virtual_size
            && let Some(bank) = bank_of(dtcm.base)
        {
            let offset = (addr - dtcm.base) % DTCM_SIZE as u32;
            return Some(Address::at(BlockId::NdsDtcm, bank, offset));
        }
        if addr >= ARM9_BIOS_BASE {
            let offset = addr - ARM9_BIOS_BASE;
            let len = buses.bios(Cpu::Arm9).len() as u32;
            return (offset < len).then(|| Address::at(BlockId::NdsBios9, 0, offset));
        }
    }
    match (addr >> 24, cpu) {
        (0x00, Cpu::Arm7) => {
            let len = buses.bios(Cpu::Arm7).len() as u32;
            (addr < len).then(|| Address::at(BlockId::NdsBios7, 0, addr))
        }
        (0x02, _) => {
            let len = buses.main_ram().len() as u32;
            at(BlockId::NdsMainRam, addr & (len - 1))
        }
        (0x03, _) => {
            let shared = buses.shared_wram(cpu).len() as u32;
            let arm7 = buses.arm7_wram().len() as u32;
            if cpu == Cpu::Arm7 && (addr >= ARM7_WRAM_BASE || shared == 0) {
                at(BlockId::NdsArm7Wram, addr & (arm7 - 1))
            } else if shared > 0 {
                at(BlockId::NdsSharedWram, addr & (shared - 1))
            } else {
                None
            }
        }
        (0x04, _) if addr - IO_BASE < io_size(cpu) => at(BlockId::NdsIo, addr - IO_BASE),
        (0x04, _) if addr.wrapping_sub(FIFO_BASE) < FIFO_SIZE => {
            at(BlockId::NdsFifo, addr - FIFO_BASE)
        }
        (0x05, Cpu::Arm9) => at(BlockId::NdsPalette, addr & (PALETTE_OAM_SIZE - 1)),
        (0x06, _) => locate_vram(buses, cpu, addr),
        (0x07, Cpu::Arm9) => at(BlockId::NdsOam, addr & (PALETTE_OAM_SIZE - 1)),
        _ => None,
    }
}

fn locate_vram(buses: &Buses, cpu: Cpu, addr: u32) -> Option<Address> {
    let purpose = match cpu {
        Cpu::Arm9 => arm9_vram_purpose(addr),
        Cpu::Arm7 => Purpose::Arm7,
    };
    let base = vram_base(cpu, purpose)?;
    let offset = (addr - base) as usize % purpose.window();
    Bank::ALL.into_iter().enumerate().find_map(|(index, bank)| {
        let (mapped, start) = buses.vram().mapping(bank)?;
        let inside = mapped == purpose && offset >= start && offset - start < bank.size();
        let first = bank_of(base + start as u32)?;
        inside.then(|| {
            Address::at(
                BlockId::NdsVram(index as u8),
                first,
                (offset - start) as u32,
            )
        })
    })
}

fn bytes(buses: &Buses, cpu: Cpu, id: BlockId) -> Option<&[u8]> {
    Some(match id {
        BlockId::NdsBios9 => buses.bios(Cpu::Arm9),
        BlockId::NdsBios7 => buses.bios(Cpu::Arm7),
        BlockId::NdsItcm => buses.tcm().itcm(),
        BlockId::NdsDtcm => buses.tcm().dtcm(),
        BlockId::NdsMainRam => buses.main_ram(),
        BlockId::NdsSharedWram => buses.shared_wram(cpu),
        BlockId::NdsArm7Wram => buses.arm7_wram(),
        BlockId::NdsPalette => buses.palette(),
        BlockId::NdsOam => buses.oam(),
        BlockId::NdsVram(index) => buses.vram().bank(*Bank::ALL.get(usize::from(index))?),
        _ => return None,
    })
}

pub(super) fn peek(buses: &Buses, cpu: Cpu, at: Address, out: &mut [u8]) {
    if let Some(bytes) = bytes(buses, cpu, at.block) {
        let start = at.offset as usize;
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = bytes.get(start + index).copied().unwrap_or(0);
        }
        return;
    }
    let Some(flat) = at.nds_address() else {
        out.fill(0);
        return;
    };
    let mut word = None;
    for (index, byte) in out.iter_mut().enumerate() {
        let addr = flat.wrapping_add(index as u32);
        let aligned = addr & !3;
        let value = match word {
            Some((at, value)) if at == aligned => value,
            _ => buses.peek_io(cpu, aligned),
        };
        word = Some((aligned, value));
        *byte = (value >> ((addr & 3) * 8)) as u8;
    }
}

pub(super) fn read(buses: &Buses, cpu: Cpu, addr: u32, width: Width) -> u32 {
    let Some(at) = locate(buses, cpu, addr) else {
        return 0;
    };
    let mut bytes = [0; 4];
    let len = width.bytes();
    peek(buses, cpu, at, &mut bytes[..len]);
    u32::from_le_bytes(bytes)
}

pub(super) fn poke(buses: &mut Buses, cpu: Cpu, at: Address, bytes: &[u8]) {
    let writable = pokeable(buses, cpu, at.block);
    let Some(flat) = at.nds_address().filter(|_| writable) else {
        return;
    };
    let tcm = matches!(at.block, BlockId::NdsItcm | BlockId::NdsDtcm);
    for (index, &byte) in bytes.iter().enumerate() {
        let offset = at.offset + index as u32;
        let mut half = [0; 2];
        peek(
            buses,
            cpu,
            Address::at(at.block, at.bank, offset & !1),
            &mut half,
        );
        half[(offset & 1) as usize] = byte;
        let value = u32::from(u16::from_le_bytes(half));
        let addr = flat.wrapping_add(index as u32) & !1;
        match (cpu, tcm) {
            (Cpu::Arm9, true) => buses.arm9().cpu_write(addr, Width::Half, value),
            (Cpu::Arm9, false) => buses.arm9().write(addr, Width::Half, value),
            (Cpu::Arm7, _) => buses.arm7().write(addr, Width::Half, value),
        }
    }
}

fn pokeable(buses: &Buses, cpu: Cpu, id: BlockId) -> bool {
    !matches!(id, BlockId::NdsBios9 | BlockId::NdsBios7) && bytes(buses, cpu, id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::vram::Bank;

    const ON: u32 = 0x80;
    const FIFO_CONTROL: u32 = 0x0400_0184;
    const FIFO_SEND: u32 = 0x0400_0188;
    const FIFO_ENABLE: u32 = 1 << 15;
    const RECEIVE_EMPTY: u32 = 1 << 8;

    fn buses() -> Buses {
        let mut buses = Buses::default();
        buses.load_arm9_bios(&[0; 0x1000]);
        buses.load_arm7_bios(&[0; 0x4000]);
        buses.boot_cp15();
        for (register, value) in [
            (0x0400_0240, ON),
            (0x0400_0242, ON | 2),
            (0x0400_0243, ON | 2 | 1 << 3),
            (0x0400_0248, ON | 1),
        ] {
            buses.arm9().write(register, Width::Byte, value);
        }
        buses
    }

    fn map(buses: &Buses, cpu: Cpu) -> Vec<(&'static str, u32, u32)> {
        let mut out = Vec::new();
        blocks(buses, cpu, &mut out);
        out.iter()
            .map(|block| (block.name, block.start, block.size))
            .collect()
    }

    #[test]
    fn the_arm9_lists_its_blocks_at_their_gbatek_addresses() {
        assert_eq!(
            map(&buses(), Cpu::Arm9),
            [
                ("ITCM", 0x0000_0000, 0x8000),
                ("Main RAM", 0x0200_0000, 0x40_0000),
                ("DTCM", 0x0300_0000, 0x4000),
                ("Shared WRAM", 0x0300_0000, 0x8000),
                ("I/O", 0x0400_0000, 0x1070),
                ("IPC FIFO and card data", 0x0410_0000, 0x20),
                ("Palette", 0x0500_0000, 0x800),
                ("VRAM H", 0x0620_0000, 0x8000),
                ("VRAM A", 0x0680_0000, 0x2_0000),
                ("OAM", 0x0700_0000, 0x800),
                ("BIOS", 0xFFFF_0000, 0x1000),
            ]
        );
    }

    #[test]
    fn the_arm7_lists_its_blocks_at_their_gbatek_addresses() {
        assert_eq!(
            map(&buses(), Cpu::Arm7),
            [
                ("BIOS", 0x0000_0000, 0x4000),
                ("Main RAM", 0x0200_0000, 0x40_0000),
                ("ARM7 WRAM", 0x0380_0000, 0x1_0000),
                ("I/O", 0x0400_0000, 0x520),
                ("IPC FIFO and card data", 0x0410_0000, 0x20),
                ("VRAM C", 0x0600_0000, 0x2_0000),
                ("VRAM D", 0x0602_0000, 0x2_0000),
            ]
        );
    }

    #[test]
    fn shared_wram_moves_between_the_cpus_with_wramcnt() {
        let mut buses = buses();
        buses.arm9().write(0x0400_0247, Width::Byte, 3);
        assert!(
            map(&buses, Cpu::Arm9)
                .iter()
                .all(|row| row.0 != "Shared WRAM")
        );
        assert!(map(&buses, Cpu::Arm7).contains(&("Shared WRAM", 0x0300_0000, 0x8000)));
        assert_eq!(
            locate(&buses, Cpu::Arm7, 0x0300_0010),
            Some(Address::at(BlockId::NdsSharedWram, 0, 0x10))
        );
        assert_eq!(
            locate(&buses, Cpu::Arm7, 0x0380_0010),
            Some(Address::at(BlockId::NdsArm7Wram, 0, 0x10))
        );
    }

    #[test]
    fn every_listed_block_locates_its_own_first_and_last_byte() {
        let buses = buses();
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let mut out = Vec::new();
            blocks(&buses, cpu, &mut out);
            for block in out
                .iter()
                .filter(|block| block.id != BlockId::NdsSharedWram)
            {
                for offset in [0, block.size - 1] {
                    let found = locate(&buses, cpu, block.start + offset);
                    assert_eq!(
                        found,
                        Some(Address::at(block.id, block.first_bank, offset)),
                        "{cpu:?} {}",
                        block.name
                    );
                    assert_eq!(
                        found.and_then(|at| at.nds_address()),
                        Some(block.start + offset)
                    );
                }
            }
        }
    }

    #[test]
    fn a_peek_reads_what_the_cpu_reads() {
        let mut buses = buses();
        buses.arm9().write(0x0200_0100, Width::Word, 0x1234_5678);
        buses.arm9().write(0x0680_0010, Width::Half, 0xBEEF);
        buses.arm7().write(0x0380_0020, Width::Word, 0xCAFE_F00D);
        assert_eq!(
            read(&buses, Cpu::Arm9, 0x0200_0100, Width::Word),
            0x1234_5678
        );
        assert_eq!(read(&buses, Cpu::Arm7, 0x0200_0102, Width::Half), 0x1234);
        assert_eq!(read(&buses, Cpu::Arm9, 0x0680_0010, Width::Half), 0xBEEF);
        assert_eq!(
            read(&buses, Cpu::Arm7, 0x0380_0020, Width::Word),
            0xCAFE_F00D
        );
    }

    #[test]
    fn a_poke_writes_one_byte_and_keeps_its_neighbour() {
        let mut buses = buses();
        buses.arm9().write(0x0500_0000, Width::Half, 0x1122);
        let at = locate(&buses, Cpu::Arm9, 0x0500_0001).unwrap();
        poke(&mut buses, Cpu::Arm9, at, &[0xAB]);
        assert_eq!(buses.arm9().read(0x0500_0000, Width::Half), 0xAB22);
    }

    #[test]
    fn peeking_ipcfiforecv_leaves_the_fifo_as_it_was() {
        let mut buses = buses();
        buses.arm9().write(FIFO_CONTROL, Width::Half, FIFO_ENABLE);
        buses.arm7().write(FIFO_CONTROL, Width::Half, FIFO_ENABLE);
        buses.arm9().write(FIFO_SEND, Width::Word, 0x1111_1111);
        buses.arm9().write(FIFO_SEND, Width::Word, 0x2222_2222);
        let at = locate(&buses, Cpu::Arm7, 0x0410_0000).unwrap();
        let mut bytes = [0; 0x20];
        for _ in 0..3 {
            peek(&buses, Cpu::Arm7, at, &mut bytes);
            assert_eq!(bytes[..4], 0x1111_1111u32.to_le_bytes());
        }
        let control = buses.arm7().read(FIFO_CONTROL, Width::Half);
        assert_eq!(control & RECEIVE_EMPTY, 0);
        assert_eq!(buses.arm7().read(0x0410_0000, Width::Word), 0x1111_1111);
        assert_eq!(buses.arm7().read(0x0410_0000, Width::Word), 0x2222_2222);
        assert_ne!(
            buses.arm7().read(FIFO_CONTROL, Width::Half) & RECEIVE_EMPTY,
            0
        );
    }

    #[test]
    fn vram_banks_list_only_where_the_cpu_sees_them() {
        let buses = buses();
        assert!(map(&buses, Cpu::Arm9).iter().all(|row| row.0 != "VRAM C"));
        assert!(map(&buses, Cpu::Arm7).iter().all(|row| row.0 != "VRAM A"));
        assert_eq!(buses.vram().mapping(Bank::C), Some((Purpose::Arm7, 0)));
    }
}
