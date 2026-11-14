use crate::digest::{Digest, Fnv, hash};
use crate::input;
use crate::manifest::{Kind, Platform, Workload};
use citrine_gb::console::{ConsoleKind, Rollback};
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::gba::machine::Gba;
use citrine_gb::gba::serial::CABLE_WINDOW;
use citrine_gb::netplay::{Advance, ConsoleSetup, Input, LinkPort, Player, Session, SessionConfig};
use citrine_gb::rom::Rom;
use std::time::{Duration, Instant};

const SAMPLE_RATE: u32 = 48_000;
const SESSION_SEED: u64 = 0x0C17_2026;
const START_TIME: u64 = 1_762_000_000;
const STATE_EVERY: usize = 30;

pub(crate) struct Outcome {
    pub(crate) digest: Digest,
    pub(crate) wall: Duration,
}

#[inline(never)]
pub(crate) fn measured<R>(work: impl FnOnce() -> R) -> R {
    std::hint::black_box(work())
}

fn timed<R>(work: impl FnOnce() -> R) -> (R, Duration) {
    let start = Instant::now();
    let out = measured(work);
    (out, start.elapsed())
}

pub(crate) fn checkpoints(frames: usize) -> [usize; 3] {
    [frames / 3, 2 * frames / 3, frames]
}

pub(crate) fn run(workload: &Workload, rom: Option<&[u8]>) -> Result<Outcome, String> {
    let frames = workload.frames as usize;
    let need_rom = || rom.ok_or_else(|| format!("{} needs a rom", workload.id));
    match (workload.kind, workload.console) {
        (Kind::Ds3d, _) => crate::ds::three_d(workload),
        (Kind::Ds2d, _) => crate::ds::two_d(workload),
        (Kind::Solo, Platform::Ds) => crate::ds::system(workload, need_rom()?),
        (_, Platform::Ds) => Err(format!("{}: the DS runs solo only", workload.id)),
        (Kind::Solo, platform) => {
            let rom = need_rom()?;
            let inputs = input::script(workload.seed, frames);
            Ok(match platform {
                Platform::Gba => solo(Gba::new(rom), &inputs),
                _ => solo(game_boy(rom, platform)?, &inputs),
            })
        }
        (Kind::State, platform) => {
            let rom = need_rom()?;
            let inputs = input::script(workload.seed, frames);
            match platform {
                Platform::Gba => state_rounds(Gba::new(rom), &inputs),
                _ => state_rounds(game_boy(rom, platform)?, &inputs),
            }
        }
        (Kind::Pair | Kind::Link | Kind::Rollback, platform) => {
            let rom = Rom::new(need_rom()?);
            let plugged = workload.kind == Kind::Link;
            let kind = match platform {
                Platform::Gb => ConsoleKind::Gb,
                Platform::Gbc => ConsoleKind::Gbc,
                _ => ConsoleKind::Gba,
            };
            let setup = || {
                Some(ConsoleSetup {
                    rom: &rom,
                    kind,
                    save: None,
                    state: None,
                })
            };
            let mut config = SessionConfig::<2> {
                cable_plugged: plugged,
                cable_window: 0,
                sample_rate: SAMPLE_RATE,
                ..SessionConfig::new(Player::ALL[0], SESSION_SEED, START_TIME)
            };
            let inputs = match workload.script.as_deref() {
                Some("petris-link") => input::petris_link(workload.seed, frames),
                _ => input::pair(workload.seed, frames, plugged),
            };
            if platform == Platform::Gba {
                if plugged {
                    config.cable_window = CABLE_WINDOW;
                }
                let session =
                    Session::new_gba(config, [setup(), setup()]).map_err(|e| e.to_string())?;
                session_run(session, &inputs, workload)
            } else {
                let session =
                    Session::new(config, [setup(), setup()]).map_err(|e| e.to_string())?;
                session_run(session, &inputs, workload)
            }
        }
    }
}

fn game_boy(rom: &[u8], platform: Platform) -> Result<GameBoy, String> {
    let model = if platform == Platform::Gbc {
        GbModel::Cgb
    } else {
        GbModel::Dmg
    };
    let mut gb = GameBoy::new_empty(model);
    gb.load_rom(&Rom::new(rom)).map_err(|e| e.to_string())?;
    Ok(gb)
}

fn solo<C: Rollback>(mut console: C, inputs: &[Input]) -> Outcome {
    console.set_sample_rate(SAMPLE_RATE);
    let marks = checkpoints(inputs.len());
    let mut audio = Vec::with_capacity(inputs.len() * 2048);
    let mut shots = Vec::new();
    let ((), wall) = timed(|| {
        let mut chunk = Vec::new();
        for (frame, input) in inputs.iter().enumerate() {
            console.set_keys(input.buttons);
            console.run_frame();
            chunk.clear();
            console.take_audio(&mut chunk);
            audio.extend_from_slice(&chunk);
            if marks.contains(&(frame + 1)) {
                shots.push(console.frame().to_vec());
            }
        }
    });
    let mut sound = Fnv::default();
    sound.samples(&audio);
    let mut scratch = console.clone_state();
    let mut facts = std::collections::BTreeMap::new();
    facts.insert("samples".to_string(), audio.len() as u64);
    Outcome {
        digest: Digest {
            frames: shots.iter().map(|shot| hash(shot)).collect(),
            audio: sound.finish(),
            state: format!("{:016x}", console.state_hash(&mut scratch)),
            facts,
        },
        wall,
    }
}

fn state_rounds<C: Rollback>(mut console: C, inputs: &[Input]) -> Result<Outcome, String> {
    console.set_sample_rate(SAMPLE_RATE);
    console.set_muted(true);
    let marks = checkpoints(inputs.len());
    let mut shots = Vec::new();
    let mut scratch = console.clone_state();
    let mut bytes = 0u64;
    let mut rounds = 0u64;
    let mut hashes = Fnv::default();
    let (result, wall) = timed(|| -> Result<(), String> {
        for (frame, input) in inputs.iter().enumerate() {
            console.set_keys(input.buttons);
            console.run_frame();
            if (frame + 1) % STATE_EVERY == 0 {
                let before = console.state_hash(&mut scratch);
                let data = console.encode_state().map_err(|e| e.to_string())?;
                bytes += data.len() as u64;
                let restored = console.decode_state(&data).map_err(|e| e.to_string())?;
                let after = restored.state_hash(&mut scratch);
                if before != after {
                    return Err(format!(
                        "state at frame {} did not survive a round trip",
                        frame + 1
                    ));
                }
                hashes.u64(after);
                console = restored;
                rounds += 1;
            }
            if marks.contains(&(frame + 1)) {
                shots.push(console.frame().to_vec());
            }
        }
        Ok(())
    });
    result?;
    let mut facts = std::collections::BTreeMap::new();
    facts.insert("state_bytes".to_string(), bytes);
    facts.insert("rounds".to_string(), rounds);
    Ok(Outcome {
        digest: Digest {
            frames: shots.iter().map(|shot| hash(shot)).collect(),
            audio: String::new(),
            state: hashes.finish(),
            facts,
        },
        wall,
    })
}

fn feed<C: Rollback + LinkPort>(
    session: &mut Session<C, 2>,
    inputs: &[Vec<Input>; 2],
    frame: usize,
) {
    let at = frame as u32;
    let mut local = inputs[0][frame];
    if let Some(pace) = session.cable_pace() {
        local = local.with_pace(pace);
    }
    session.add_local_input(at, local).ok();
    session.add_player_input(Player::ALL[1], at, inputs[1][frame]);
}

fn session_run<C: Rollback + LinkPort>(
    mut session: Session<C, 2>,
    inputs: &[Vec<Input>; 2],
    workload: &Workload,
) -> Result<Outcome, String> {
    let frames = inputs[0].len();
    let marks = checkpoints(frames);
    let depth = workload.depth as usize;
    let mut audio = Vec::new();
    let mut shots = Vec::new();
    let mut rollbacks = 0u64;
    let mut resimulated = 0u64;
    let (result, wall) = timed(|| -> Result<(), String> {
        let mut chunk = Vec::new();
        let mut step = |session: &mut Session<C, 2>| -> Result<(), String> {
            match session.advance() {
                Advance::Ran => {}
                other => return Err(format!("the session did not run a frame: {other:?}")),
            }
            chunk.clear();
            session.drain_audio(&mut chunk);
            audio.extend_from_slice(&chunk);
            while session.take_check().is_some() {}
            if session.last_rollback() > 0 {
                rollbacks += 1;
                resimulated += u64::from(session.last_rollback());
            }
            Ok(())
        };
        let mut remote_next = 0;
        for frame in 0..frames {
            if depth == 0 {
                feed(&mut session, inputs, frame);
            } else {
                let mut local = inputs[0][frame];
                if let Some(pace) = session.cable_pace() {
                    local = local.with_pace(pace);
                }
                session.add_local_input(frame as u32, local).ok();
                if (frame + 1) % (depth + 1) == 0 || frame + 1 == frames {
                    for (at, input) in inputs[1]
                        .iter()
                        .enumerate()
                        .take(frame + 1)
                        .skip(remote_next)
                    {
                        session.add_player_input(Player::ALL[1], at as u32, *input);
                    }
                    remote_next = frame + 1;
                }
            }
            step(&mut session)?;
            if marks.contains(&(frame + 1)) {
                let mut shot = Fnv::default();
                for player in &Player::ALL[..2] {
                    shot.bytes(session.console(*player).frame());
                }
                shots.push(shot.finish());
            }
        }
        Ok(())
    });
    result?;
    if let Some(desync) = session.desync() {
        return Err(format!("{}: desync {desync:?}", workload.id));
    }
    let mut sound = Fnv::default();
    sound.samples(&audio);
    let mut state = Fnv::default();
    for player in &Player::ALL[..2] {
        let console = session.console(*player);
        let mut scratch = console.clone_state();
        state.u64(console.state_hash(&mut scratch));
    }
    let mut facts = std::collections::BTreeMap::new();
    facts.insert("samples".to_string(), audio.len() as u64);
    facts.insert(
        "cable_transfers".to_string(),
        session.cable_transfers().iter().sum(),
    );
    if depth > 0 {
        facts.insert("rollbacks".to_string(), rollbacks);
        facts.insert("resimulated".to_string(), resimulated);
    }
    Ok(Outcome {
        digest: Digest {
            frames: shots,
            audio: sound.finish(),
            state: state.finish(),
            facts,
        },
        wall,
    })
}
