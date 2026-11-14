#![expect(
    clippy::disallowed_methods,
    reason = "the bench times a handoff between two plain threads, outside citrine-gb-threads"
)]

use citrine_gb::console::{Console, ConsoleKind, Keys, Rollback};
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::gba::machine::Gba;
use citrine_gb::gba::serial::CABLE_WINDOW;
use citrine_gb::netplay::{
    Advance, CableEvent, ConsoleSetup, Input, LinkPort, Player, Session, SessionConfig,
};
use citrine_gb::rom::Rom;
use citrine_gb_threads::{Mode, Threads};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WARMUP: usize = 120;
const SEED: u64 = 0x0C17_2026;
const START_TIME: u64 = 1_762_000_000;

thread_local! {
    static THREADS: Threads = Threads::new(Mode::from_env());
}

struct Bench {
    dir: PathBuf,
    frames: usize,
    extra: Option<PathBuf>,
    only: Option<String>,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

const PAD: [Keys; 7] = [
    Keys::empty(),
    Keys::LEFT,
    Keys::RIGHT,
    Keys::UP,
    Keys::DOWN,
    Keys::A,
    Keys::B,
];

fn script(seed: u64, frames: usize) -> Vec<Input> {
    let mut rng = Rng(seed | 1);
    let mut inputs = Vec::with_capacity(frames);
    while inputs.len() < frames {
        let keys = if inputs.len() % 240 < 6 {
            Keys::START
        } else {
            PAD[(rng.next() % PAD.len() as u64) as usize]
        };
        let length = 1 + (rng.next() % 20) as usize;
        inputs.extend(std::iter::repeat_n(Input::new(keys), length));
    }
    inputs.truncate(frames);
    inputs
}

fn petris_link(frames: usize) -> [Vec<Input>; 2] {
    let mut inputs = [
        vec![Input::default(); frames],
        vec![Input::default(); frames],
    ];
    let mut press = |side: usize, at: usize, keys: Keys, length: usize| {
        for input in inputs[side].iter_mut().skip(at).take(length) {
            input.buttons = keys;
        }
    };
    press(0, 450, Keys::START, 5);
    press(1, 450, Keys::START, 5);
    press(0, 600, Keys::DOWN, 5);
    press(0, 640, Keys::RIGHT, 5);
    press(0, 700, Keys::START, 5);
    let pad = [Keys::LEFT, Keys::RIGHT, Keys::A, Keys::DOWN, Keys::empty()];
    let mut rng = Rng(9);
    for side in 0..2 {
        let mut at = 800;
        while at < frames {
            let length = (4 + (rng.next() % 20) as usize).min(frames - at);
            press(
                side,
                at,
                pad[(rng.next() % pad.len() as u64) as usize],
                length,
            );
            at += length;
        }
    }
    inputs[0][0].cable = Some(CableEvent::Plug);
    inputs
}

struct Stats {
    median: f64,
    p99: f64,
    worst: f64,
    mean: f64,
}

fn stats(times: &mut [Duration]) -> Stats {
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let at = |q: f64| ms(times[((times.len() as f64 * q) as usize).min(times.len() - 1)]);
    let total: Duration = times.iter().sum();
    Stats {
        median: at(0.5),
        p99: at(0.99),
        worst: ms(times[times.len() - 1]),
        mean: ms(total) / times.len() as f64,
    }
}

fn row(case: &str, times: &mut [Duration]) {
    let s = stats(times);
    println!(
        "| {case} | {:.3} | {:.3} | {:.3} | {:.3} | {} |",
        s.median,
        s.p99,
        s.worst,
        s.mean,
        times.len()
    );
}

fn read_rom(path: &Path) -> Vec<u8> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if path.extension().is_some_and(|e| e == "br") {
        let mut out = Vec::new();
        brotli::BrotliDecompress(&mut data.as_slice(), &mut out).unwrap();
        out
    } else {
        data
    }
}

fn gb_setup(rom: &Rom) -> Option<ConsoleSetup<'_>> {
    Some(ConsoleSetup {
        rom,
        kind: GbModel::for_rom(rom).unwrap().into(),
        save: None,
        state: None,
    })
}

fn gba_setup(rom: &Rom) -> Option<ConsoleSetup<'_>> {
    Some(ConsoleSetup {
        rom,
        kind: ConsoleKind::Gba,
        save: None,
        state: None,
    })
}

fn config<const N: usize>(plugged: bool) -> SessionConfig<N> {
    SessionConfig {
        cable_plugged: plugged,
        cable_window: 0,
        ..SessionConfig::new(Player::ALL[0], SEED, START_TIME)
    }
}

fn windowed<const N: usize>() -> SessionConfig<N> {
    SessionConfig {
        cable_window: CABLE_WINDOW,
        ..config(true)
    }
}

fn console_frames(console: &mut impl Console, inputs: &[Input]) -> Vec<Duration> {
    let mut times = Vec::with_capacity(inputs.len());
    for (frame, input) in inputs.iter().enumerate() {
        console.set_keys(input.buttons);
        let start = Instant::now();
        console.run_frame();
        let took = start.elapsed();
        let mut audio = Vec::new();
        console.take_audio(&mut audio);
        if frame >= WARMUP {
            times.push(took);
        }
    }
    times
}

fn feed<C: Rollback + LinkPort, const N: usize>(
    session: &mut Session<C, N>,
    inputs: &[Vec<Input>; N],
    frame: usize,
) {
    let at = frame as u32;
    let mut input = inputs[0][frame];
    if let Some(pace) = session.cable_pace() {
        input = input.with_pace(pace);
    }
    session.add_local_input(at, input).unwrap();
    for (player, script) in Player::ALL.iter().zip(inputs).skip(1) {
        session.add_player_input(*player, at, script[frame]);
    }
}

fn step<C: Rollback + LinkPort, const N: usize>(session: &mut Session<C, N>, audio: &mut Vec<f32>) {
    let advance = THREADS.with(|threads| threads.split(|split| session.advance_on(split)));
    assert!(matches!(advance, Advance::Ran));
    audio.clear();
    session.drain_audio(audio);
    while session.take_check().is_some() {}
}

fn session_frames<C: Rollback + LinkPort, const N: usize>(
    session: &mut Session<C, N>,
    inputs: &[Vec<Input>; N],
) -> Vec<Duration> {
    let frames = inputs[0].len();
    let mut audio = Vec::new();
    let mut times = Vec::with_capacity(frames);
    for frame in 0..frames {
        feed(session, inputs, frame);
        let start = Instant::now();
        step(session, &mut audio);
        if frame >= WARMUP {
            times.push(start.elapsed());
        }
    }
    times
}

fn replays<C: Rollback + LinkPort, const N: usize>(
    session: &mut Session<C, N>,
    frames: usize,
    depth: usize,
) -> Vec<Duration> {
    let local = script(1, frames + depth + 1);
    let mut audio = Vec::new();
    let mut times = Vec::new();
    let mut held = Keys::empty();
    let mut remote_next = 0;
    let mut frame = 0;
    let mut event = 0u32;
    while frame + depth < frames {
        for (at, input) in local.iter().enumerate().skip(frame).take(depth) {
            session.add_local_input(at as u32, *input).unwrap();
            step(session, &mut audio);
        }
        let fresh = if event.is_multiple_of(2) {
            Keys::A
        } else {
            Keys::B
        };
        for at in remote_next..frame + depth {
            let keys = if at < frame { held } else { fresh };
            for player in Player::ALL.iter().take(N).skip(1) {
                session.add_player_input(*player, at as u32, Input::new(keys));
            }
        }
        held = fresh;
        remote_next = frame + depth;
        session
            .add_local_input((frame + depth) as u32, local[frame + depth])
            .unwrap();
        let start = Instant::now();
        step(session, &mut audio);
        let took = start.elapsed();
        if frame >= WARMUP {
            times.push(took);
        }
        assert!(
            session.last_rollback() as usize >= depth,
            "rolled back {} frames",
            session.last_rollback()
        );
        frame += depth + 1;
        event += 1;
    }
    times
}

struct Turns {
    steps: u64,
    switches: u64,
    messages: u64,
}

fn linked_turns<C: Console + LinkPort, const N: usize>(
    consoles: &mut [C; N],
    keys: impl Fn(usize, usize) -> Keys,
    frames: usize,
) -> Turns {
    for (unit, console) in consoles.iter_mut().enumerate() {
        console.plug_link_as(unit, N);
    }
    let mut clocks = [0u64; N];
    let mut in_flight: [VecDeque<(u64, C::Message)>; N] = std::array::from_fn(|_| VecDeque::new());
    let mut turns = Turns {
        steps: 0,
        switches: 0,
        messages: 0,
    };
    let mut audio = Vec::new();
    for frame in 0..frames {
        for (unit, console) in consoles.iter_mut().enumerate() {
            console.set_keys(keys(unit, frame));
            clocks[unit] += console.begin_linked_frame();
        }
        let mut done = [false; N];
        let mut last = usize::MAX;
        while let Some(i) = (0..N).filter(|&i| !done[i]).min_by_key(|&i| clocks[i]) {
            let console = &mut consoles[i];
            while let Some(&(due, _)) = in_flight[i].front() {
                if due > clocks[i] {
                    break;
                }
                let (_, message) = in_flight[i].pop_front().unwrap();
                console.receive_link_message(message);
            }
            let alone = (0..N).all(|j| j == i || done[j]);
            if alone && console.link_waiting() {
                console.end_linked_frame();
                done[i] = true;
                continue;
            }
            clocks[i] += console.step_linked();
            if frame >= WARMUP {
                turns.steps += 1;
                turns.switches += u64::from(last != i);
            }
            last = i;
            for message in console.take_link_messages() {
                if frame >= WARMUP {
                    turns.messages += 1;
                }
                for (to, queue) in in_flight.iter_mut().enumerate() {
                    if to != i {
                        queue.push_back((clocks[i], message.clone()));
                    }
                }
            }
            if console.linked_frame_done() {
                console.end_linked_frame();
                done[i] = true;
            }
        }
        for console in consoles.iter_mut() {
            audio.clear();
            console.take_audio(&mut audio);
        }
    }
    turns
}

fn turns_row(case: &str, turns: &Turns, frames: usize) {
    let per = |n: u64| n as f64 / frames as f64;
    println!(
        "| {case} | {:.0} | {:.0} | {:.1} |",
        per(turns.steps),
        per(turns.switches),
        per(turns.messages)
    );
}

#[cfg(not(target_os = "wasi"))]
fn handoffs(rounds: u32) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;

    let turn = Arc::new(AtomicU32::new(0));
    let other = Arc::clone(&turn);
    let partner = std::thread::spawn(move || {
        for _ in 0..rounds {
            while other.load(Ordering::Acquire) != 1 {
                std::hint::spin_loop();
            }
            other.store(0, Ordering::Release);
        }
    });
    let start = Instant::now();
    for _ in 0..rounds {
        turn.store(1, Ordering::Release);
        while turn.load(Ordering::Acquire) != 0 {
            std::hint::spin_loop();
        }
    }
    let spin = start.elapsed();
    partner.join().unwrap();

    let (to_partner, partner_inbox) = mpsc::channel::<u32>();
    let (to_main, main_inbox) = mpsc::channel::<u32>();
    let partner = std::thread::spawn(move || {
        while let Ok(n) = partner_inbox.recv() {
            if to_main.send(n).is_err() {
                break;
            }
        }
    });
    let start = Instant::now();
    for n in 0..rounds {
        to_partner.send(n).unwrap();
        main_inbox.recv().unwrap();
    }
    let parked = start.elapsed();
    drop(to_partner);
    partner.join().unwrap();

    let ns = |d: Duration| d.as_secs_f64() * 1e9 / f64::from(rounds * 2);
    println!("| spin on an atomic | {:.0} |", ns(spin));
    println!("| mpsc channel, blocking recv | {:.0} |", ns(parked));
}

#[cfg(target_os = "wasi")]
fn handoffs(_rounds: u32) {
    println!("| no threads on wasm32-wasip1 | - |");
}

fn extra_sessions(name: &str, rom: &Rom, frames: usize) {
    header("sessions, every input on time");
    let inputs = [script(5, frames), script(7, frames)];
    let mut pair = Session::new_gba(config(false), [gba_setup(rom), gba_setup(rom)]).unwrap();
    row(
        &format!("{name} pair, unplugged"),
        &mut session_frames(&mut pair, &inputs),
    );
    let group_inputs = [
        script(5, frames),
        script(7, frames),
        script(11, frames),
        script(13, frames),
    ];
    let mut group =
        Session::<Gba, 4>::new_gba(config(true), std::array::from_fn(|_| gba_setup(rom))).unwrap();
    row(
        &format!("{name} group of 4, plugged"),
        &mut session_frames(&mut group, &group_inputs),
    );
    header("replay of 8 frames plus 1 new frame");
    let mut pair = Session::new_gba(config(false), [gba_setup(rom), gba_setup(rom)]).unwrap();
    row(
        &format!("{name} pair, unplugged"),
        &mut replays(&mut pair, frames, 8),
    );
}

fn header(title: &str) {
    println!("\n### {title}\n");
    println!("| case | median ms | p99 ms | worst ms | mean ms | frames |");
    println!("|---|---|---|---|---|---|");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let bench = Bench {
        dir: PathBuf::from(args.next().expect("homebrew dir")),
        frames: args.next().map_or(3600, |n| n.parse().unwrap()),
        extra: args.next().filter(|p| !p.is_empty()).map(PathBuf::from),
        only: args.next(),
    };
    let wants = |part: &str| bench.only.as_deref().is_none_or(|only| only.contains(part));
    let total = WARMUP + bench.frames;
    THREADS.with(|threads| {
        println!(
            "threads: {:?}, pool of {}",
            threads.mode(),
            threads.pool_size()
        );
    });
    if bench
        .only
        .as_deref()
        .is_some_and(|only| only.starts_with("single"))
    {
        let path = bench.extra.as_deref().expect("a rom for the single run");
        let data = read_rom(path);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        header("1 console, alone (run_frame only)");
        if data.len() > 0x200 && path.to_string_lossy().contains(".gba") {
            row(
                &name,
                &mut console_frames(&mut Gba::new(&data), &script(3, total)),
            );
            if wants("sessions") {
                extra_sessions(&name, &Rom::new(&data), WARMUP + bench.frames / 3);
            }
        } else {
            let rom = Rom::new(&data);
            let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).unwrap());
            gb.load_rom(&rom).unwrap();
            row(&name, &mut console_frames(&mut gb, &script(3, total)));
        }
        return;
    }
    let rom = |name: &str| Rom::new(&read_rom(&bench.dir.join(name)));
    let tobu = rom("tobu.gb");
    let ucity = rom("ucity.gbc");
    let petris = rom("petris.gbc");
    let gba_names = [
        "pentris",
        "hyperspace_roll",
        "cascade7",
        "gbarcade",
        "nonogram_advance",
        "the_hat_chooses_the_wizard",
    ];
    let gba_roms: Vec<(String, Vec<u8>)> = gba_names
        .iter()
        .map(|name| {
            (
                name.to_string(),
                read_rom(&bench.dir.join(format!("gba/{name}.gba.br"))),
            )
        })
        .chain(bench.extra.iter().map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, read_rom(path))
        }))
        .collect();
    let main_gba = Rom::new(&gba_roms[0].1);

    if wants("alone") {
        header("1 console, alone (run_frame only)");
        for (name, model_rom) in [
            ("GB tobu", &tobu),
            ("GBC ucity", &ucity),
            ("GBC petris", &petris),
        ] {
            let mut gb = GameBoy::new_empty(GbModel::for_rom(model_rom).unwrap());
            gb.load_rom(model_rom).unwrap();
            row(name, &mut console_frames(&mut gb, &script(3, total)));
        }
        for (name, data) in &gba_roms {
            let mut gba = Gba::new(data);
            row(
                &format!("GBA {name}"),
                &mut console_frames(&mut gba, &script(3, total)),
            );
        }
    }

    if wants("session") {
        header("sessions, every input on time (advance: ring save, frame, checks)");
        let inputs = [script(5, total), script(7, total)];
        let mut pair = Session::new(config(false), [gb_setup(&tobu), gb_setup(&tobu)]).unwrap();
        row(
            "GB pair tobu, unplugged",
            &mut session_frames(&mut pair, &inputs),
        );
        let link = petris_link(total);
        let mut pair = Session::new(config(true), [gb_setup(&petris), gb_setup(&petris)]).unwrap();
        row(
            "GBC pair petris, plugged, linked play",
            &mut session_frames(&mut pair, &link),
        );
        println!(
            "\npetris transfers per console: {:?}\n",
            pair.cable_transfers()
        );
        let mut pair =
            Session::new_gba(config(false), [gba_setup(&main_gba), gba_setup(&main_gba)]).unwrap();
        row(
            "GBA pair pentris, unplugged",
            &mut session_frames(&mut pair, &inputs),
        );
        let mut pair =
            Session::new_gba(config(true), [gba_setup(&main_gba), gba_setup(&main_gba)]).unwrap();
        row(
            "GBA pair pentris, plugged",
            &mut session_frames(&mut pair, &inputs),
        );
        let group_inputs = [
            script(5, total),
            script(7, total),
            script(11, total),
            script(13, total),
        ];
        let setups = || std::array::from_fn(|_| gba_setup(&main_gba));
        let mut group = Session::<Gba, 4>::new_gba(config(false), setups()).unwrap();
        row(
            "GBA group of 4 pentris, unplugged",
            &mut session_frames(&mut group, &group_inputs),
        );
        let mut group = Session::<Gba, 4>::new_gba(config(true), setups()).unwrap();
        row(
            "GBA group of 4 pentris, plugged",
            &mut session_frames(&mut group, &group_inputs),
        );
        let mut group = Session::<Gba, 4>::new_gba(windowed(), setups()).unwrap();
        row(
            "GBA group of 4 pentris, plugged, in windows",
            &mut session_frames(&mut group, &group_inputs),
        );
    }

    if wants("room") {
        header("room: your game, a pair session and a watched group of 4, one after another");
        let hyper = Rom::new(&gba_roms[1].1);
        let mut own = Gba::new(&hyper.data);
        let link = petris_link(total);
        let mut pair = Session::new(config(true), [gb_setup(&petris), gb_setup(&petris)]).unwrap();
        let group_inputs = [
            script(5, total),
            script(7, total),
            script(11, total),
            script(13, total),
        ];
        let mut group =
            Session::<Gba, 4>::new_gba(config(true), std::array::from_fn(|_| gba_setup(&main_gba)))
                .unwrap();
        let own_inputs = script(3, total);
        let mut audio = Vec::new();
        let mut times = Vec::new();
        for (frame, input) in own_inputs.iter().enumerate() {
            feed(&mut pair, &link, frame);
            feed(&mut group, &group_inputs, frame);
            let start = Instant::now();
            own.set_keys(input.buttons);
            own.run_frame();
            audio.clear();
            own.take_audio(&mut audio);
            step(&mut pair, &mut audio);
            step(&mut group, &mut audio);
            if frame >= WARMUP {
                times.push(start.elapsed());
            }
        }
        row(
            "GBA hyperspace_roll + GBC petris pair plugged + GBA group of 4 plugged",
            &mut times,
        );
    }

    if wants("replay") {
        header("replay of 8 frames plus 1 new frame (the advance after late inputs)");
        let mut pair = Session::new(config(false), [gb_setup(&tobu), gb_setup(&tobu)]).unwrap();
        row("GB pair tobu, unplugged", &mut replays(&mut pair, total, 8));
        let mut pair = Session::new(config(true), [gb_setup(&tobu), gb_setup(&tobu)]).unwrap();
        row("GB pair tobu, plugged", &mut replays(&mut pair, total, 8));
        let mut pair =
            Session::new_gba(config(false), [gba_setup(&main_gba), gba_setup(&main_gba)]).unwrap();
        row(
            "GBA pair pentris, unplugged",
            &mut replays(&mut pair, total, 8),
        );
        let setups = || std::array::from_fn(|_| gba_setup(&main_gba));
        let mut group = Session::<Gba, 4>::new_gba(config(false), setups()).unwrap();
        row(
            "GBA group of 4 pentris, unplugged",
            &mut replays(&mut group, total, 8),
        );
        let mut group = Session::<Gba, 4>::new_gba(config(true), setups()).unwrap();
        row(
            "GBA group of 4 pentris, plugged",
            &mut replays(&mut group, total, 8),
        );
        let mut group = Session::<Gba, 4>::new_gba(windowed(), setups()).unwrap();
        row(
            "GBA group of 4 pentris, plugged, in windows",
            &mut replays(&mut group, total, 8),
        );
    }

    if wants("link") {
        println!("\n### linked consoles: instruction turns per frame\n");
        println!(
            "| case | steps per frame | console switches per frame | link messages per frame |"
        );
        println!("|---|---|---|---|");
        let link = petris_link(total);
        let mut pair = [
            GameBoy::new_empty(GbModel::Cgb),
            GameBoy::new_empty(GbModel::Cgb),
        ];
        for gb in &mut pair {
            gb.load_rom(&petris).unwrap();
        }
        let turns = linked_turns(&mut pair, |unit, frame| link[unit][frame].buttons, total);
        turns_row("GBC pair petris, linked play", &turns, bench.frames);
        let mut group: [Gba; 4] = std::array::from_fn(|_| Gba::new(&main_gba.data));
        let scripts = [
            script(5, total),
            script(7, total),
            script(11, total),
            script(13, total),
        ];
        let turns = linked_turns(
            &mut group,
            |unit, frame| scripts[unit][frame].buttons,
            total,
        );
        turns_row("GBA group of 4 pentris", &turns, bench.frames);
        println!("\n### cross-thread handoff, one way\n");
        println!("| how | ns per handoff |");
        println!("|---|---|");
        handoffs(200_000);
    }
}
