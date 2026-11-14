use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [rom, out] = args.as_slice() else {
        eprintln!("usage: extract <game.nds> <out dir>");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(rom) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("{rom}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let tables = match citrine_gb_pkm::tables::extract(&bytes) {
        Ok(tables) => tables,
        Err(err) => {
            eprintln!("{rom}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let dir = PathBuf::from(out);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("{out}: {err}");
        return ExitCode::FAILURE;
    }
    for (name, text) in tables.to_tsv() {
        if let Err(err) = std::fs::write(dir.join(name), text) {
            eprintln!("{name}: {err}");
            return ExitCode::FAILURE;
        }
    }
    println!(
        "{:?} {:?}: {} species, {} encounters",
        tables.game,
        tables.language,
        tables.base_stats.len(),
        tables.encounters.len()
    );
    ExitCode::SUCCESS
}
