use super::gen3::{Gen3Save, Pocket, TestMon3, gen3_flash};
use super::gen3_rom::{SPOTS, gen3_rom};
use crate::gen3::Layout;
use crate::{GameState, Mon, Pokemon, Tables};

pub const GEN3_OT: u32 = 0xD431_3039;
pub const EWRAM: usize = 0x4_0000;
pub const IWRAM: usize = 0x8000;
const KEY: u32 = 0xA5C3_1E77;
const SMALL_AT: usize = 0x2_A000;
const LARGE_AT: usize = 0x2_B000;
const PC_AT: usize = 0x3_0000;

pub fn rom_code(layout: Layout) -> &'static [u8; 4] {
    match layout {
        Layout::RubySapphire => b"AXVE",
        Layout::Emerald => b"BPEE",
        Layout::FireRedLeafGreen => b"BPRE",
    }
}

pub fn gen3_tables(layout: Layout) -> Tables {
    Tables::load(&gen3_rom(rom_code(layout), 0, SPOTS)).expect("tables")
}

pub fn treecko() -> TestMon3 {
    TestMon3::new(277, 0x1234_5678, GEN3_OT)
}

pub fn torchic() -> TestMon3 {
    TestMon3::new(280, 0x0BAD_F00D, GEN3_OT)
}

pub fn gen3_sample_save(layout: Layout) -> Gen3Save {
    let key = if layout == Layout::RubySapphire {
        0
    } else {
        KEY
    };
    let mut save = Gen3Save::new(layout, key);
    save.trainer("BRENDAN", false, (12345, 54321), (123, 45, 6));
    save.party(&[treecko().party(), torchic().party()]);
    save.pocket(Pocket::Berries, &[(133, 3)]);
    save.put_box(0, 3, &treecko().boxed());
    save.put_box(
        0,
        7,
        &TestMon3 {
            egg: true,
            ..TestMon3::new(281, 0x0000_0042, GEN3_OT)
        }
        .boxed(),
    );
    for number in 0..14 {
        save.box_name(number, &format!("BOX{number}"));
    }
    save.story(true, 3, false);
    save
}

pub fn gen3_battery(save: &Gen3Save) -> Vec<u8> {
    gen3_flash(Some(&save.slot(1, 0)), None)
}

pub fn gen3_state(layout: Layout, save: &Gen3Save) -> GameState {
    gen3_tables(layout)
        .read_save(&gen3_battery(save))
        .expect("save")
}

pub fn hatched(pokemon: &Pokemon) -> &Mon {
    match pokemon {
        Pokemon::Hatched(mon) => mon,
        Pokemon::Egg => panic!("egg"),
    }
}

pub fn put(memory: &mut [u8], at: usize, bytes: &[u8]) {
    memory[at..at + bytes.len()].copy_from_slice(bytes);
}

pub fn gen3_running(layout: Layout, save: &Gen3Save, live_party: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    let mut ewram = vec![0; EWRAM];
    let mut iwram = vec![0; IWRAM];
    let party: Vec<u8> = live_party.concat();
    match layout {
        Layout::RubySapphire => {
            put(&mut ewram, SMALL_AT, &save.small);
            put(&mut ewram, SMALL_AT + 0x890, &save.large);
            put(&mut ewram, PC_AT, &save.pc);
            put(&mut iwram, 0x4360, &party);
        }
        Layout::Emerald | Layout::FireRedLeafGreen => {
            put(&mut ewram, SMALL_AT, &save.small);
            put(&mut ewram, LARGE_AT, &save.large);
            put(&mut ewram, PC_AT, &save.pc);
            let pointers = if layout == Layout::Emerald {
                0x5D8C
            } else {
                0x5008
            };
            let words: Vec<u8> = [LARGE_AT, SMALL_AT, PC_AT]
                .iter()
                .flat_map(|&at| (0x0200_0000 + at as u32).to_le_bytes())
                .collect();
            put(&mut iwram, pointers, &words);
            put(&mut ewram, 0x2_44EC, &party);
        }
    }
    (ewram, iwram)
}
