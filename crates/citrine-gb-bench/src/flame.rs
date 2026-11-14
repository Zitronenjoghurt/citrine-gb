use crate::callgrind::Profile;
use inferno::flamegraph::{self, Options};
use std::io::{BufReader, Cursor};
use std::path::Path;

const SMALLEST: f64 = 0.0005;

fn options(title: &str) -> Options<'static> {
    let mut options = Options::default();
    options.title = title.to_string();
    options.count_name = "instructions".to_string();
    options.min_width = 0.05;
    options
}

pub(crate) fn render(
    title: &str,
    head: &Path,
    base: Option<&Path>,
    out: &Path,
) -> Result<(), String> {
    let read = |path: &Path| -> Result<Vec<String>, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Profile::parse(&text).folded(SMALLEST))
    };
    let after = read(head)?;
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut options = options(title);
    match base {
        None => flamegraph::from_lines(&mut options, after.iter().map(String::as_str), file)
            .map_err(|e| e.to_string()),
        Some(base) => {
            let before = read(base)?;
            let mut merged = Vec::new();
            inferno::differential::from_readers(
                inferno::differential::Options::default(),
                BufReader::new(Cursor::new(before.join("\n"))),
                BufReader::new(Cursor::new(after.join("\n"))),
                &mut merged,
            )
            .map_err(|e| e.to_string())?;
            let text = String::from_utf8(merged).map_err(|e| e.to_string())?;
            flamegraph::from_lines(&mut options, text.lines(), file).map_err(|e| e.to_string())
        }
    }
}
