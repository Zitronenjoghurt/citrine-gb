use crate::console::{Console, ConsoleKind, Keys};
use crate::debug::Debug;
use crate::debug::registers::RegisterGroup;
use crate::debug::step::{Step, Stop};
use crate::gb::{GameBoy, GbModel};
use crate::machine::{Firmware, Machine};
use crate::persistence::state::{GBA_STATE_MAGIC, SAVE_QUALITY, StateBody, state_kind};
use crate::rom::Rom;

const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");
const PETRIS: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/petris.gbc");
const NOW: u64 = 1_700_000_000;
const FRAMES: u32 = 600;

fn keys_at(frame: u32) -> Keys {
    match frame % 120 {
        0..10 => Keys::START,
        30..40 => Keys::A,
        50..90 => Keys::RIGHT,
        95..100 => Keys::A | Keys::LEFT,
        _ => Keys::empty(),
    }
}

fn game_boy(data: &[u8], model: GbModel) -> GameBoy {
    let mut gb = GameBoy::new_empty(model);
    gb.load_rom_with_model(&Rom::new(data), model)
        .expect("load");
    gb.apu.set_sample_rate(48_000);
    gb
}

fn machine(data: &[u8], kind: ConsoleKind) -> Machine {
    let mut machine = Machine::load(&Rom::new(data), kind, &Firmware::default()).expect("load");
    machine.set_sample_rate(48_000);
    machine
}

#[test]
fn a_machine_plays_like_the_game_boy_it_holds() {
    let mut gb = game_boy(TOBU, GbModel::Dmg);
    let mut machine = machine(TOBU, ConsoleKind::Gb);
    let mut audio = Vec::new();
    let mut heard = 0;

    for frame in 0..FRAMES {
        gb.set_keys(keys_at(frame));
        machine.set_keys(keys_at(frame));
        gb.run_frame();
        machine.run_frame();

        assert_eq!(machine.frame(), gb.frame().as_slice(), "frame {frame}");
        machine.take_audio(&mut audio);
        let expected = std::mem::take(&mut gb.apu.audio_buffer);
        assert_eq!(audio, expected, "audio of frame {frame}");
        heard += audio.len();
        audio.clear();
    }

    assert!(heard > 0);
    assert_eq!(
        machine.save_state(NOW).expect("machine state"),
        gb.dump_full(NOW).expect("game boy state")
    );
}

#[test]
fn a_machine_from_a_state_goes_on_like_the_game_boy() {
    let mut gb = game_boy(TOBU, GbModel::Dmg);
    for frame in 0..300 {
        gb.set_keys(keys_at(frame));
        gb.run_frame();
    }
    let state = gb.dump_full(NOW).expect("state");

    let rom = Rom::new(TOBU);
    let mut machine =
        Machine::from_state(&state, &rom, NOW, &Firmware::default()).expect("machine");
    let mut again = GameBoy::from_dump(&state, &rom, NOW).expect("game boy");
    for frame in 300..FRAMES {
        machine.set_keys(keys_at(frame));
        again.set_keys(keys_at(frame));
        machine.run_frame();
        again.run_frame();
    }

    assert_eq!(machine.frame(), again.frame().as_slice());
    assert_eq!(
        machine.save_state(NOW).expect("machine state"),
        again.dump_full(NOW).expect("game boy state")
    );
}

#[test]
fn a_game_boy_describes_its_console() {
    let dmg = machine(TOBU, ConsoleKind::Gb);
    let cgb = machine(PETRIS, ConsoleKind::Gbc);

    assert_eq!(dmg.kind(), ConsoleKind::Gb);
    assert_eq!(cgb.kind(), ConsoleKind::Gbc);
    assert_eq!((dmg.video().width, dmg.video().height), (160, 144));
    assert_eq!(dmg.timing().clock_hz, 4_194_304);
    assert_eq!(dmg.timing().cycles_per_frame, 70_224);
    assert_eq!(
        dmg.audio_channels(),
        ["Pulse 1", "Pulse 2", "Wave", "Noise"]
    );
    assert_eq!(dmg.rom_id(), Rom::new(TOBU).header().expect("header").id());
    assert!(dmg.as_gb().is_some_and(|gb| gb.model == GbModel::Dmg));
    assert!(cgb.as_gb().is_some_and(|gb| gb.model == GbModel::Cgb));
}

#[cfg(not(feature = "gba"))]
#[test]
fn a_gba_game_does_not_load_into_a_machine() {
    assert!(Machine::load(&Rom::new(TOBU), ConsoleKind::Gba, &Firmware::default()).is_err());
}

#[cfg(feature = "gba")]
#[test]
fn a_gba_game_loads_into_a_gba_machine() {
    let mut machine = Machine::load(
        &Rom::new(&[0xFE, 0xFF, 0xFF, 0xEA]),
        ConsoleKind::Gba,
        &Firmware::default(),
    )
    .expect("load");
    machine.run_frame();
    assert_eq!(machine.kind(), ConsoleKind::Gba);
    assert_eq!(machine.video().width, 240);
    assert_eq!(machine.frame().len(), 240 * 160 * 4);
    assert!(machine.as_gb().is_none());
}

#[test]
fn a_boot_rom_from_the_firmware_runs_first() {
    let firmware = Firmware {
        gb_boot_rom: Some(vec![0x00; 0x100]),
        #[cfg(feature = "gba")]
        gba_bios: None,
        #[cfg(feature = "gba")]
        gba_boot: crate::gba::bios::Boot::Skip,
    };
    let machine = Machine::load(&Rom::new(TOBU), ConsoleKind::Gb, &firmware).expect("load");
    let gb = machine.as_gb().expect("game boy");

    assert!(gb.boot_rom.mounted);
    assert_eq!(gb.cpu.pc, 0x0000);
}

#[test]
fn a_muted_channel_is_muted_through_the_machine() {
    let mut machine = machine(TOBU, ConsoleKind::Gb);
    machine.set_channel_enabled(2, false);

    let gb = machine.as_gb().expect("game boy");
    assert!(!gb.apu.channel_enabled(2));
    assert!(gb.apu.channel_enabled(1));
}

#[test]
fn the_debug_view_of_a_machine_is_its_game_boy() {
    let mut gb = game_boy(TOBU, GbModel::Dmg);
    let mut machine = machine(TOBU, ConsoleKind::Gb);
    gb.debugger.record_channels = true;
    machine.record_waves(true);
    gb.debugger.static_analysis_enabled = true;
    machine.set_code_analysis(true);
    assert!(machine.code_analysis());
    for frame in 0..FRAMES {
        gb.set_keys(keys_at(frame));
        machine.set_keys(keys_at(frame));
        gb.run_frame();
        machine.run_frame();
    }

    let mut expected: Vec<RegisterGroup> = Vec::new();
    gb.registers(&mut expected);
    let mut groups = Vec::new();
    Debug::registers(&machine, &mut groups);
    assert_eq!(groups, expected);

    assert_eq!(Debug::pc(&machine), gb.pc_address());
    assert_eq!(Debug::code_rows(&machine), gb.code_rows());
    assert!(gb.code_rows() > 0);
    let mut lines = Vec::new();
    let mut expected_lines = Vec::new();
    Debug::disassemble(&machine, 0..gb.code_rows(), &mut lines);
    gb.disassemble(0..gb.code_rows(), &mut expected_lines);
    assert_eq!(lines, expected_lines);
    let pc = gb.pc_address().expect("pc");
    assert_eq!(Debug::code_row_of(&machine, pc), gb.code_row_of(pc));

    assert_eq!(Debug::cycles(&machine), gb.debugger.total_cycles);
    assert!(!gb.debugger.waves().is_empty());
    assert_eq!(machine.waves(), gb.debugger.waves());
}

#[test]
fn breakpoints_set_through_the_machine_stop_its_game_boy() {
    let mut machine = machine(TOBU, ConsoleKind::Gb);
    machine.run_frame();
    let pc = Debug::pc(&machine).expect("pc");
    machine.breakpoints().insert(pc);

    let gb = machine.as_gb().expect("game boy");
    assert!(gb.debugger.breakpoints.contains(&pc));
}

#[test]
fn a_game_boy_has_its_interrupt_vectors_and_a_short_pc() {
    let machine = machine(TOBU, ConsoleKind::Gb);
    let features = machine.features();

    assert_eq!(features.pc_digits, 4);
    assert!(!features.thumb);
    assert_eq!(
        features.vectors,
        [
            ("VBlank", 0x40),
            ("STAT", 0x48),
            ("Timer", 0x50),
            ("Serial", 0x58),
            ("Joypad", 0x60),
        ]
    );
}

#[test]
fn a_step_through_the_machine_steps_its_game_boy() {
    let mut gb = game_boy(TOBU, GbModel::Dmg);
    let mut machine = machine(TOBU, ConsoleKind::Gb);
    gb.run_frame();
    machine.run_frame();
    let before = Debug::cycles(&machine);

    assert_eq!(Debug::step(&mut machine, Step::Instruction), Stop::Finished);
    assert_eq!(gb.run_step(Step::Instruction), Stop::Finished);

    assert!(Debug::cycles(&machine) > before);
    assert_eq!(Debug::cycles(&machine), Debug::cycles(&gb));
    assert_eq!(Debug::pc(&machine), Debug::pc(&gb));
}

fn cart(cartridge_type: u8, ram_size_code: u8) -> Machine {
    let mut data = vec![0u8; 0x8000];
    data[0x0134..0x0143].copy_from_slice(b"CITRINE CARTRGE");
    data[0x0147] = cartridge_type;
    data[0x0149] = ram_size_code;
    machine(&data, ConsoleKind::Gb)
}

#[test]
fn a_machine_knows_its_rom_title_battery_and_clock() {
    let tobu = machine(TOBU, ConsoleKind::Gb);
    assert_eq!(
        tobu.rom_bytes(),
        game_boy(TOBU, GbModel::Dmg).cartridge.rom_bytes()
    );
    assert!(!tobu.has_clock());

    let plain = cart(0x03, 0x02);
    assert_eq!(plain.title(), "CITRINE CARTRGE");
    assert!(plain.has_battery());
    assert!(!plain.has_clock());

    let clock = cart(0x10, 0x02);
    assert!(clock.has_battery());
    assert!(clock.has_clock());
}

#[test]
fn a_state_body_packs_to_the_saved_state() {
    let mut machine = machine(PETRIS, ConsoleKind::Gbc);
    for frame in 0..120 {
        machine.set_keys(keys_at(frame));
        machine.run_frame();
    }
    let body = machine.state_body(NOW).expect("body");
    assert_eq!(body.kind, ConsoleKind::Gbc);
    assert_eq!(
        body.pack(SAVE_QUALITY).expect("pack"),
        machine.save_state(NOW).expect("state")
    );
}

#[test]
fn a_state_tells_its_console() {
    let mut dmg = machine(TOBU, ConsoleKind::Gb);
    let mut cgb = machine(PETRIS, ConsoleKind::Gbc);
    let mut cgb_as_dmg = machine(PETRIS, ConsoleKind::Gb);
    assert_eq!(
        state_kind(&dmg.save_state(NOW).unwrap()),
        Some(ConsoleKind::Gb)
    );
    assert_eq!(
        state_kind(&cgb.save_state(NOW).unwrap()),
        Some(ConsoleKind::Gbc)
    );
    assert_eq!(
        state_kind(&cgb_as_dmg.save_state(NOW).unwrap()),
        Some(ConsoleKind::Gb)
    );
    assert_eq!(state_kind(b"not a state"), None);
}

#[test]
fn a_gba_state_starts_with_its_magic_and_version() {
    let body = StateBody {
        kind: ConsoleKind::Gba,
        data: b"gba body".to_vec(),
    };
    let state = body.pack(SAVE_QUALITY).expect("pack");
    assert_eq!(state[..4], GBA_STATE_MAGIC);
    assert_eq!(state[4], 1);
    assert_eq!(state_kind(&state), Some(ConsoleKind::Gba));
}

#[test]
fn a_game_boy_state_has_no_magic() {
    let mut machine = machine(TOBU, ConsoleKind::Gb);
    let state = machine.save_state(NOW).expect("state");
    assert_ne!(state[..4], GBA_STATE_MAGIC);
    let body = machine.state_body(NOW).expect("body");
    assert_eq!(body.pack(SAVE_QUALITY).expect("pack"), state);
}

#[cfg(not(feature = "gba"))]
#[test]
fn a_gba_state_does_not_load_as_a_game_boy() {
    let state = StateBody {
        kind: ConsoleKind::Gba,
        data: Vec::new(),
    }
    .pack(SAVE_QUALITY)
    .expect("pack");
    let refused = Machine::from_state(&state, &Rom::new(TOBU), NOW, &Firmware::default())
        .err()
        .expect("refused");
    assert!(matches!(refused, crate::error::GbError::GbaGame));
}
