use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use citrine_walker_lab::harness;
use citrine_walker_lab::script::{self, Step};
use citrine_walker_lab::walker::{CLOCK_HZ, EEPROM_SIZE, HEIGHT, WIDTH, Walker};

const SCALE: usize = 4;
const GREYS: [u8; 4] = [0xE0, 0xA0, 0x60, 0x20];

struct Args {
    rom: PathBuf,
    eeprom: Option<PathBuf>,
    fill: u8,
    script: PathBuf,
    out: PathBuf,
    save_eeprom: Option<PathBuf>,
}

fn args() -> Result<Args, String> {
    let mut rom = std::env::var_os("CITRINE_WALKER_ROM").map(PathBuf::from);
    let mut eeprom = std::env::var_os("CITRINE_WALKER_EEPROM").map(PathBuf::from);
    let mut fill = 0xFF;
    let mut script = None;
    let mut out = PathBuf::from("walker-out");
    let mut save_eeprom = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--rom" => rom = Some(value()?.into()),
            "--eeprom" => eeprom = Some(value()?.into()),
            "--fill" => fill = u8::from_str_radix(&value()?, 16).map_err(|e| e.to_string())?,
            "--script" => script = Some(value()?.into()),
            "--out" => out = value()?.into(),
            "--save-eeprom" => save_eeprom = Some(value()?.into()),
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    Ok(Args {
        rom: rom.ok_or("no ROM: pass --rom or set CITRINE_WALKER_ROM")?,
        eeprom,
        fill,
        script: script.ok_or("no --script")?,
        out,
        save_eeprom,
    })
}

fn write_frame(path: &Path, frame: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(
        BufWriter::new(file),
        (WIDTH * SCALE) as u32,
        (HEIGHT * SCALE) as u32,
    );
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    let mut data = Vec::with_capacity(WIDTH * HEIGHT * SCALE * SCALE);
    for y in 0..HEIGHT * SCALE {
        for x in 0..WIDTH * SCALE {
            data.push(GREYS[frame[(y / SCALE) * WIDTH + x / SCALE] as usize & 3]);
        }
    }
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&data))
        .map_err(|e| e.to_string())
}

fn hex(bytes: impl IntoIterator<Item = u8>) -> String {
    bytes
        .into_iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn run(a: &Args) -> Result<(), String> {
    let rom = std::fs::read(&a.rom).map_err(|e| format!("{}: {e}", a.rom.display()))?;
    let eeprom = match &a.eeprom {
        Some(p) => std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?,
        None => vec![a.fill; EEPROM_SIZE],
    };
    let steps = script::parse(&std::fs::read_to_string(&a.script).map_err(|e| e.to_string())?)?;
    std::fs::create_dir_all(&a.out).map_err(|e| e.to_string())?;
    let mut w = Walker::new(rom, eeprom);
    let fault = |w: &Walker, f| format!("{f:?} recent pcs {:04x?}", w.recent_pcs());
    for step in steps {
        let t = w.now() as f64 / CLOCK_HZ as f64;
        match step {
            Step::Wait(s) => w.run_seconds(s).map_err(|f| fault(&w, f))?,
            Step::Press(b) => w.press(b).map_err(|f| fault(&w, f))?,
            Step::Hold(b, s) => {
                w.set_button(b, true);
                w.run_seconds(s).map_err(|f| fault(&w, f))?;
                w.set_button(b, false);
            }
            Step::Walk(n, rate) => w.walk(n, rate),
            Step::Frame(name) => {
                write_frame(&a.out.join(format!("{name}.png")), &w.frame())?;
                println!("{t:8.2}s frame {name}");
            }
            Step::Ir(bytes) => w.ir_receive(&bytes),
            Step::IrLog => println!(
                "{t:8.2}s ir out: {}",
                hex(w.ir_take().into_iter().map(|(_, b)| b))
            ),
            Step::Identify => match harness::identify(&mut w) {
                Ok(id) => println!(
                    "{t:8.2}s identify: walker session {:08x}, reply cmd {:02x} extra {:02x}, {} payload bytes: {}",
                    id.walker_session,
                    id.reply.cmd,
                    id.reply.extra,
                    id.reply.payload.len(),
                    hex(id.reply.payload.iter().copied())
                ),
                Err(e) => println!("{t:8.2}s identify failed: {e:?}"),
            },
            Step::Connect(cmds) => {
                let cmds: Vec<(u8, u8, Vec<u8>)> =
                    cmds.into_iter().map(|(c, e)| (c, e, Vec::new())).collect();
                match harness::connect(&mut w, &cmds) {
                    Ok(replies) => {
                        for r in replies {
                            println!(
                                "{t:8.2}s reply cmd {:02x} extra {:02x} session {:08x}: {}",
                                r.cmd,
                                r.extra,
                                r.session,
                                hex(r.payload.iter().copied())
                            );
                        }
                    }
                    Err(e) => println!("{t:8.2}s connect failed: {e:?}"),
                }
            }
            Step::EepromReads => {
                let reads = w.take_eeprom_reads();
                let text: Vec<String> = reads
                    .iter()
                    .map(|(a, n)| format!("{a:04x}+{n:x}"))
                    .collect();
                println!("{t:8.2}s eeprom reads: {}", text.join(" "));
            }
            Step::Trace(lo, hi) => w.board.io_trace = Some((lo, hi, Vec::new())),
            Step::TraceDump => {
                if let Some((_, _, log)) = w.board.io_trace.take() {
                    for e in log {
                        println!(
                            "{:10} pc {:04x} {} {:04x} {:02x}",
                            e.at,
                            e.pc,
                            if e.write { "W" } else { "R" },
                            e.addr,
                            e.value
                        );
                    }
                }
            }
            Step::Count(addr) => w.hits.push((addr, 0)),
            Step::Counts => {
                for (at, count) in &w.hits {
                    println!("{t:8.2}s count {at:04x}: {count}");
                }
            }
            Step::Ram(addr, len) => {
                let bytes: Vec<u8> = (0..len).map(|i| w.read(addr.wrapping_add(i))).collect();
                println!("{t:8.2}s ram {addr:04x}: {}", hex(bytes));
            }
        }
    }
    println!(
        "pc {:04x} ccr {:02x} sleeping {} watchdog resets {} eeprom writes {} ir overruns {} dropped {}",
        w.cpu.pc,
        w.cpu.ccr,
        w.cpu.sleeping,
        w.watchdog_resets.len(),
        w.eeprom_writes(),
        w.ir_overruns(),
        w.ir_dropped()
    );
    let resets: Vec<String> = w
        .watchdog_resets
        .iter()
        .map(|&t| format!("{:.2}", t as f64 / CLOCK_HZ as f64))
        .collect();
    println!("watchdog resets at: {}", resets.join(" "));
    println!(
        "accel regs: {}",
        hex(w.accel_regs().iter().take(0x18).copied())
    );
    println!(
        "lcd commands (first 64): {}",
        hex(w.lcd_commands().iter().take(64).copied())
    );
    for (addr, c) in &w.board.unknown_io {
        println!("io {addr:04x} reads {} writes {}", c.reads, c.writes);
    }
    if let Some(p) = &a.save_eeprom {
        std::fs::write(p, w.eeprom()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match args().and_then(|a| run(&a)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("walker-lab: {e}");
            ExitCode::FAILURE
        }
    }
}
