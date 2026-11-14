use crate::debug::registers::{Register, RegisterGroup};
use crate::nds::Cpu;
use crate::nds::bus::Buses;
use crate::nds::io_names::{self, IoName};
use std::sync::OnceLock;

const WORD: u32 = 4;

struct WordName {
    cpu: Cpu,
    addr: u32,
    name: &'static str,
}

fn word_names() -> &'static [WordName] {
    static NAMES: OnceLock<Vec<WordName>> = OnceLock::new();
    NAMES.get_or_init(|| {
        [Cpu::Arm9, Cpu::Arm7]
            .into_iter()
            .flat_map(|cpu| {
                io_names::groups(cpu)
                    .iter()
                    .flat_map(|group| group.rows)
                    .filter(|row| u32::from(row.size) > WORD)
                    .flat_map(move |row| {
                        (0..u32::from(row.size).div_ceil(WORD)).map(move |index| WordName {
                            cpu,
                            addr: row.addr + index * WORD,
                            name: format!("{} {index}", row.name).leak(),
                        })
                    })
            })
            .collect()
    })
}

fn word_name(cpu: Cpu, addr: u32) -> &'static str {
    word_names()
        .iter()
        .find(|word| word.cpu == cpu && word.addr == addr)
        .map_or("", |word| word.name)
}

pub(super) fn value(buses: &Buses, cpu: Cpu, addr: u32, size: u32) -> u32 {
    let word = buses.peek_io(cpu, addr & !3);
    let shifted = word >> ((addr & 3) * 8);
    match size {
        1 => shifted & 0xFF,
        2 => shifted & 0xFFFF,
        _ => shifted,
    }
}

fn rows(buses: &Buses, cpu: Cpu, row: &IoName, out: &mut Vec<Register>) {
    let size = u32::from(row.size);
    if size <= WORD {
        out.push(Register::new(
            row.name,
            value(buses, cpu, row.addr, size),
            (size * 8) as u8,
        ));
        return;
    }
    for start in (0..size).step_by(WORD as usize) {
        let addr = row.addr + start;
        let width = (size - start).min(WORD);
        out.push(Register::new(
            word_name(cpu, addr),
            value(buses, cpu, addr, width),
            (width * 8) as u8,
        ));
    }
}

pub(super) fn registers(buses: &Buses, cpu: Cpu, out: &mut Vec<RegisterGroup>) {
    for group in io_names::groups(cpu) {
        let mut registers = Vec::new();
        for row in group.rows {
            rows(buses, cpu, row, &mut registers);
        }
        out.push(RegisterGroup {
            name: group.name,
            rows: registers,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::wait::Width;
    use crate::nds::machine::tests::nds;

    const BRANCH_TO_SELF: u32 = 0xEAFF_FFFE;
    const SIDE_EFFECTS: [u32; 2] = [0x0410_0000, 0x0410_0010];

    fn width(size: u8) -> Width {
        match size {
            1 => Width::Byte,
            2 => Width::Half,
            _ => Width::Word,
        }
    }

    #[test]
    fn each_register_shows_what_its_cpu_reads() {
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        nds.run_frame();
        nds.run_frame();
        for cpu in [Cpu::Arm9, Cpu::Arm7] {
            let mut groups = Vec::new();
            registers(nds.buses(), cpu, &mut groups);
            let shown: Vec<_> = groups.iter().flat_map(|group| &group.rows).collect();
            let named = io_names::groups(cpu)
                .iter()
                .flat_map(|group| group.rows)
                .filter(|row| row.size <= 4 && !SIDE_EFFECTS.contains(&row.addr));
            for row in named {
                let register = shown
                    .iter()
                    .find(|register| register.name == row.name)
                    .unwrap();
                let buses = nds.buses_mut();
                let read = match cpu {
                    Cpu::Arm9 => buses.arm9().read(row.addr, width(row.size)),
                    Cpu::Arm7 => buses.arm7().read(row.addr, width(row.size)),
                };
                assert_eq!(register.value, read, "{cpu:?} {}", row.name);
                assert_eq!(register.bits, row.size * 8);
            }
        }
    }

    #[test]
    fn groups_follow_the_gbatek_parts_of_each_cpu() {
        let nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        let mut groups = Vec::new();
        registers(nds.buses(), Cpu::Arm7, &mut groups);
        let names: Vec<_> = groups.iter().map(|group| group.name).collect();
        for part in [
            "DMA", "Timers", "IPC", "Keypad", "SPI", "Card", "Sound", "Power",
        ] {
            assert!(names.contains(&part), "{part}");
        }
    }

    #[test]
    fn wide_registers_split_into_numbered_words() {
        let nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        let mut groups = Vec::new();
        registers(nds.buses(), Cpu::Arm9, &mut groups);
        let math = groups.iter().find(|group| group.name == "Math").unwrap();
        let names: Vec<_> = math.rows.iter().map(|row| row.name).collect();
        assert!(names.contains(&"DIV_NUMER 0") && names.contains(&"DIV_NUMER 1"));
        assert!(math.rows.iter().all(|row| row.bits <= 32));
    }
}
