pub(crate) mod flight;
pub mod log_ring;

use crate::app::ui_state::settings::Settings;
use crate::controller_debug::SystemInfo;
use crate::emulator::Emulator;
use crate::storage::now_unix;
use citrine_gb::console::{Console, ConsoleKind, VideoSpec};
use citrine_gb::persistence::state::StateBody;
use citrine_gb_report::{
    AppInfo, FirmwareInfo, FrameInput, GameInfo, Manifest, ReplayInfo, Report, ReportError, Screen,
    frame_digest, traffic_text,
};

pub(crate) const FEATURES: [(&str, bool); 5] = [
    ("gba", cfg!(feature = "gba")),
    ("nds", cfg!(feature = "nds")),
    ("audio-codec", cfg!(feature = "audio-codec")),
    ("dev-route", cfg!(feature = "dev-route")),
    ("profile", cfg!(feature = "profile")),
];

const PRIVATE_SETTINGS: [&str; 3] = ["synced", "discord", "achievements"];
const PRIVATE_NESTED: [(&str, &str); 3] = [
    ("online", "relays"),
    ("online", "joined"),
    ("ds_online", "dns"),
];

pub(crate) struct Built {
    pub(crate) file_name: String,
    pub(crate) zip: Vec<u8>,
}

struct Picture {
    rgba: Vec<u8>,
    video: VideoSpec,
    seconds_before: u32,
}

pub(crate) struct Gathered {
    file_name: String,
    manifest: Manifest,
    state: Option<StateBody>,
    inputs: Vec<FrameInput>,
    save: Option<Vec<u8>>,
    pictures: Vec<Picture>,
    settings: Option<String>,
    log: String,
    traffic: String,
}

pub(crate) fn gather(emulator: &mut Emulator, settings: &Settings) -> Gathered {
    let now = now_unix();
    let file_name = format!(
        "{} report.zip",
        crate::media::file_stem(emulator.game_name(), now)
    );
    let save = emulator.export_save_bytes();
    let firmware = firmware_info(&emulator.firmware);
    let frames_played = emulator.total_frames as u64;
    emulator.finish_picture();
    let core = emulator.core_mut();
    let machine = &core.machine;
    let rom = machine.rom_bytes();
    let id = machine.rom_id();
    let (state, inputs, replay) = match core.flight.recorded() {
        Some(recorded) => {
            let body = &recorded.checkpoint.body;
            let state = StateBody {
                kind: body.kind,
                data: body.data.clone(),
            };
            let replay = ReplayInfo {
                state_unix: recorded.checkpoint.now,
                frames: recorded.inputs.len() as u64,
                end_digest: format!("{:016x}", frame_digest(machine.frame())),
            };
            (Some(state), recorded.inputs, Some(replay))
        }
        None => (None, Vec::new(), None),
    };
    let shots: Vec<_> = core.flight.shots().collect();
    let mut pictures: Vec<Picture> = shots
        .iter()
        .enumerate()
        .map(|(index, shot)| Picture {
            rgba: shot.rgba.clone(),
            video: shot.video,
            seconds_before: (shots.len() - index) as u32,
        })
        .collect();
    pictures.push(Picture {
        rgba: machine.frame().to_vec(),
        video: machine.video(),
        seconds_before: 0,
    });
    let system = SystemInfo::current(String::new());
    Gathered {
        file_name,
        manifest: Manifest {
            created_unix: now,
            app: AppInfo {
                version: system.version.to_string(),
                core_version: citrine_gb::VERSION.to_string(),
                features: FEATURES
                    .iter()
                    .filter(|(_, on)| *on)
                    .map(|(name, _)| name.to_string())
                    .collect(),
                target: system.build,
                os: system.os,
                profile: if cfg!(debug_assertions) {
                    "debug".into()
                } else {
                    "release".into()
                },
            },
            game: GameInfo {
                console: console_name(machine.kind()).into(),
                title: machine.title(),
                code: game_code(machine.kind(), &rom),
                crc32: format!("{:08x}", id.crc32),
                sha1: hex(&id.sha1),
                frames_played,
            },
            firmware,
            replay,
            ..Manifest::default()
        },
        state,
        inputs,
        save,
        pictures,
        settings: settings_json(settings),
        log: log_ring::text(),
        traffic: traffic_text(&machine.traffic()),
    }
}

impl Gathered {
    pub(crate) fn build(self) -> Result<Built, ReportError> {
        let state = match &self.state {
            Some(body) => Some(body.pack(body.quality(true))?),
            None => None,
        };
        let screens = self
            .pictures
            .iter()
            .map(|picture| {
                Screen::from_rgba(
                    &picture.rgba,
                    picture.video.width,
                    picture.video.height,
                    picture.seconds_before,
                )
            })
            .collect::<Result<_, _>>()?;
        let report = Report {
            manifest: self.manifest,
            state,
            inputs: self.inputs,
            save: self.save,
            screens,
            settings: self.settings,
            log: self.log,
            traffic: self.traffic,
        };
        Ok(Built {
            file_name: self.file_name,
            zip: report.to_zip()?,
        })
    }
}

fn firmware_info(firmware: &citrine_gb::machine::Firmware) -> FirmwareInfo {
    let sha1 = |bytes: &[u8]| hex(&citrine_gb::rom::id::RomId::new(bytes).sha1);
    #[cfg_attr(not(feature = "gba"), expect(unused_mut))]
    let mut info = FirmwareInfo {
        gb_boot_rom_sha1: firmware.gb_boot_rom.as_deref().map(sha1),
        ..FirmwareInfo::default()
    };
    #[cfg(feature = "gba")]
    {
        info.gba_bios_sha1 = firmware.gba_bios.as_ref().map(|bios| sha1(bios.image()));
    }
    #[cfg(feature = "nds")]
    {
        info.nds_arm9_bios_sha1 = firmware.nds.arm9_bios.as_deref().map(sha1);
        info.nds_arm7_bios_sha1 = firmware.nds.arm7_bios.as_deref().map(sha1);
        info.nds_firmware_sha1 = firmware.nds.firmware.as_deref().map(sha1);
        info.nds_user = firmware.nds.user_settings.clone();
    }
    info
}

fn settings_json(settings: &Settings) -> Option<String> {
    serde_json::to_string_pretty(&shareable_settings(settings)?).ok()
}

pub(crate) fn shareable_settings(settings: &Settings) -> Option<serde_json::Value> {
    let mut value = serde_json::to_value(settings).ok()?;
    if let Some(object) = value.as_object_mut() {
        for key in PRIVATE_SETTINGS {
            object.remove(key);
        }
        for (parent, key) in PRIVATE_NESTED {
            if let Some(parent) = object.get_mut(parent).and_then(|v| v.as_object_mut()) {
                parent.remove(key);
            }
        }
    }
    Some(value)
}

pub(crate) fn console_name(kind: ConsoleKind) -> &'static str {
    match kind {
        ConsoleKind::Gb => "gb",
        ConsoleKind::Gbc => "gbc",
        ConsoleKind::Gba => "gba",
        ConsoleKind::Nds => "nds",
    }
}

pub(crate) fn game_code(kind: ConsoleKind, rom: &[u8]) -> Option<String> {
    let range = match kind {
        ConsoleKind::Nds => 0x0C..0x10,
        ConsoleKind::Gba => 0xAC..0xB0,
        ConsoleKind::Gb | ConsoleKind::Gbc => return None,
    };
    let code = rom.get(range)?;
    code.iter()
        .all(u8::is_ascii_alphanumeric)
        .then(|| String::from_utf8_lossy(code).into_owned())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::flight::CHECKPOINT_FRAMES;
    use super::*;
    use citrine_gb::console::Keys;
    #[cfg(feature = "nds")]
    use citrine_gb::console::Touch;
    use citrine_gb::gb::{GameBoy, GbModel};
    use citrine_gb::machine::{Firmware, Machine};
    use citrine_gb::rom::Rom;
    use citrine_gb_report::replay;
    use citrine_gb_threads::{Mode, Threads};

    const TOBU: &[u8] = include_bytes!("../homebrew/tobu.gb");

    fn script(frame: usize) -> Keys {
        match frame % 90 {
            0..10 => Keys::START,
            20..40 => Keys::A,
            50..70 => Keys::RIGHT | Keys::B,
            _ => Keys::empty(),
        }
    }

    fn play(emulator: &mut Emulator, frames: std::ops::Range<usize>) {
        for frame in frames {
            emulator.debug_mut().set_keys(script(frame));
            assert_eq!(emulator.lend_one_frame(), 1, "frame {frame}");
        }
    }

    fn reported(emulator: &mut Emulator) -> Report {
        let mut settings = Settings::default();
        settings.ds_online.dns = Some("10.20.30.40".into());
        let built = gather(emulator, &settings).build().unwrap();
        assert!(
            built.file_name.ends_with(" report.zip"),
            "{}",
            built.file_name
        );
        let report = Report::from_zip(&built.zip).unwrap();
        let settings = report.settings.as_deref().unwrap();
        assert!(!settings.contains("\"relays\""));
        assert!(!settings.contains("\"synced\""));
        assert!(!settings.contains("10.20.30.40"));
        report
    }

    fn threads() -> Threads {
        Threads::new(Mode::Threads)
    }

    fn on(threads: &Threads, machine: Machine) -> Emulator {
        let mut emulator = Emulator::default();
        emulator.run_on(threads);
        emulator.put_machine(machine);
        emulator
    }

    fn tobu(threads: &Threads) -> Emulator {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&Rom::new(TOBU)).unwrap();
        on(threads, Machine::Gb(Box::new(gb)))
    }

    #[test]
    fn a_game_boy_report_replays_through_a_soft_reset_to_the_same_last_frame() {
        let threads = threads();
        let mut emulator = tobu(&threads);
        play(&mut emulator, 0..CHECKPOINT_FRAMES + 90);
        emulator.soft_reset(&egui::Context::default());
        play(&mut emulator, 0..400);
        let report = reported(&mut emulator);

        assert_eq!(report.manifest.game.console, "gb");
        assert_eq!(report.inputs.len(), CHECKPOINT_FRAMES + 490);
        assert_eq!(report.inputs.iter().filter(|input| input.reset).count(), 1);
        assert_eq!(report.screens.len(), 6);
        let replayed = replay(&report, &Rom::new(TOBU), &Firmware::default()).unwrap();
        assert_eq!(replayed.frames as usize, report.inputs.len());
        assert!(replayed.matches);
    }

    #[test]
    fn a_report_after_a_state_load_replays_only_from_the_load() {
        let threads = threads();
        let mut emulator = tobu(&threads);
        play(&mut emulator, 0..200);
        let state = emulator.debug_mut().state_body(1_790_000_000).unwrap();
        play(&mut emulator, 200..300);
        let rom = Rom::new(TOBU);
        let packed = state.pack(state.quality(false)).unwrap();
        let restored =
            Machine::from_state(&packed, &rom, 1_790_000_000, &Firmware::default()).unwrap();
        emulator.put_machine(restored);
        play(&mut emulator, 0..50);
        let report = reported(&mut emulator);

        assert_eq!(report.inputs.len(), 50);
        let replayed = replay(&report, &rom, &Firmware::default()).unwrap();
        assert!(replayed.matches);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_report_replays_touches_and_keeps_the_device_traffic() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/nds/blocksds-console-both-screens.nds");
        let data = std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        let rom = Rom::new(&data);
        let machine = Machine::load(&rom, ConsoleKind::Nds, &Firmware::default()).unwrap();
        let threads = threads();
        let mut emulator = on(&threads, machine);
        play(&mut emulator, 0..120);
        emulator.set_touch(Some(Touch { x: 120, y: 80 }));
        play(&mut emulator, 120..130);
        emulator.set_touch(None);
        play(&mut emulator, 130..200);
        let report = reported(&mut emulator);

        assert_eq!(report.manifest.game.console, "nds");
        assert_eq!(report.inputs.len(), 200);
        assert_eq!(
            report
                .inputs
                .iter()
                .filter(|input| input.touch.is_some())
                .count(),
            10
        );
        assert!(report.traffic.lines().count() > 1, "{}", report.traffic);
        let replayed = replay(&report, &rom, &Firmware::default()).unwrap();
        assert!(replayed.matches);
    }
}
