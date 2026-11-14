use crate::app::tabs::{TabViewer, playing};
use crate::app::widgets::threads_debug::{Group, Line, Row, ThreadsView, facts, millis};
use crate::icons;
use citrine_gb_net::OnlineId;
use citrine_gb_threads::{Kind, Threads, Unthreaded};
use egui::Widget;
use web_time::Instant;

const JIT_THREAD: &str = "jit";

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let threads = viewer.threads;
    let state = &mut viewer.ui.threads_debug;
    state.mark_seen();
    state.update(Instant::now(), || threads.readings());
    ui.ctx().request_repaint();
    let measured = state.measured();
    let mut rows = state.rows().to_vec();
    rows.sort_by_key(|row| (order(&row.reading.name, row.reading.kind), row.reading.id));
    let console_away = rows
        .iter()
        .any(|row| row.reading.name == "console" && !row.reading.inline);
    let mut app = vec![main_line(viewer, console_away)];
    app.extend(sound_line(viewer));
    let mut games = Vec::new();
    let mut background = Vec::new();
    let mut jits = 0;
    for row in &rows {
        let mut heading = title(viewer, row);
        if row.reading.name == JIT_THREAD {
            jits += 1;
            heading = format!("{heading} {jits}");
        }
        let line = Line {
            title: heading,
            runs: runs(viewer, row, measured),
            busy: measured.then_some(row.busy),
            facts: facts(row, measured),
        };
        match row.reading.kind {
            Kind::Worker if row.reading.name == "saves" => background.push(line),
            Kind::Worker | Kind::Pool => games.push(line),
            Kind::Side if row.reading.name == JIT_THREAD => games.push(line),
            Kind::Background | Kind::Side => background.push(line),
        }
    }
    let threads_running =
        1 + usize::from(
            viewer
                .audio
                .as_ref()
                .is_some_and(|audio| audio.is_running()),
        ) + rows.iter().filter(|row| !row.reading.inline).count();
    ThreadsView {
        summary: summary(threads, threads_running),
        groups: vec![
            Group {
                icon: icons::APP_WINDOW,
                title: "App",
                hint: "The main thread draws the screen and reads input. Sound plays on a thread the system or browser runs.",
                lines: app,
            },
            Group {
                icon: icons::GAME_CONTROLLER,
                title: "Games",
                hint: "The console runs the game you play. Sessions and watched rooms run a friend's game. Helpers take parts of a frame off them, such as a DS game's 3D render or a linked console. JIT compilers turn a DS game's code into code for this device while it plays.",
                lines: games,
            },
            Group {
                icon: icons::CLOCK_COUNTDOWN,
                title: "Background",
                hint: "Saves, file reads and other short jobs. A short job shows only while it runs.",
                lines: background,
            },
        ],
    }
    .ui(ui);
}

pub fn summary(threads: &Threads, running: usize) -> String {
    let cores = threads.cores();
    let cores = if cores == 1 {
        "1 core".to_string()
    } else {
        format!("{cores} cores")
    };
    match threads.unthreaded() {
        None if cfg!(target_arch = "wasm32") => {
            format!("{running} threads on {cores}, as web workers")
        }
        None => format!("{running} threads on {cores}"),
        Some(Unthreaded::NotIsolated) => {
            "1 thread: this page isn't cross-origin isolated, so the browser allows no workers"
                .to_string()
        }
        Some(Unthreaded::NoThreadBuild) => "1 thread: this build has no threads".to_string(),
        Some(Unthreaded::Chosen) => "1 thread: everything runs on the main thread".to_string(),
    }
}

fn main_line(viewer: &TabViewer, console_away: bool) -> Line {
    let main = viewer.ui.threads_debug.main_row();
    Line {
        title: "Main".to_string(),
        runs: if console_away {
            "Screen, input and the app".to_string()
        } else {
            "Screen, input, the app and the game".to_string()
        },
        busy: main.map(|main| main.busy),
        facts: main.map_or_else(
            || "Measuring".to_string(),
            |main| {
                format!(
                    "{} a frame, {:.0} frames a second",
                    millis(main.per_frame),
                    main.frames_per_sec
                )
            },
        ),
    }
}

fn sound_line(viewer: &TabViewer) -> Option<Line> {
    let audio = viewer.audio.as_ref().filter(|audio| audio.is_running())?;
    let rate = audio.get_sample_rate().max(1);
    let queued = viewer
        .emulator
        .audio_feed()
        .map_or(0, |feed| feed.queued_frames());
    let queued_ms = queued as u64 * 1000 / u64::from(rate);
    let gaps = audio.stats().underruns();
    let gaps = if gaps == 1 {
        "1 gap".to_string()
    } else {
        format!("{gaps} gaps")
    };
    Some(Line {
        title: "Sound".to_string(),
        runs: if cfg!(target_arch = "wasm32") {
            "The browser's sound thread".to_string()
        } else {
            "The system's sound thread".to_string()
        },
        busy: None,
        facts: format!("{queued_ms} ms of sound queued, {gaps} since it started"),
    })
}

fn friend(viewer: &TabViewer, id: &str) -> String {
    id.parse::<OnlineId>().map_or_else(
        |_| id.to_string(),
        |id| playing::full_name(viewer.play, viewer.online, id),
    )
}

fn title(viewer: &TabViewer, row: &Row) -> String {
    let name = row.reading.name.as_str();
    if let Some(n) = name.strip_prefix("pool ") {
        return n
            .parse::<usize>()
            .map_or_else(|_| "Helper".to_string(), |n| format!("Helper {}", n + 1));
    }
    if let Some(id) = name.strip_prefix("session: ") {
        return format!("Session with {}", friend(viewer, id));
    }
    if let Some(id) = name.strip_prefix("watch: ") {
        return format!("Watching {}", friend(viewer, id));
    }
    match name {
        "console" => "Console".to_string(),
        JIT_THREAD => "JIT compiler".to_string(),
        "saves" => "Saves".to_string(),
        "engine build" | "group engine build" => "Session start".to_string(),
        "shelf lookup" => "Game shelf".to_string(),
        "game check" => "Game check".to_string(),
        "codec download" => "Video codec download".to_string(),
        "media read" => "Media read".to_string(),
        "import" => "Data import".to_string(),
        "webcam" => "Camera".to_string(),
        other => {
            let mut text = other.to_string();
            if let Some(first) = text.get(..1) {
                text = first.to_uppercase() + &text[1..];
            }
            text
        }
    }
}

fn runs(viewer: &TabViewer, row: &Row, measured: bool) -> String {
    let name = row.reading.name.as_str();
    match row.reading.kind {
        Kind::Pool if !measured => String::new(),
        Kind::Pool => helper_runs(&row.tasks),
        Kind::Worker if name == "console" => {
            let game = viewer.emulator.game_name();
            if game.is_empty() {
                "No game".to_string()
            } else {
                game.to_string()
            }
        }
        Kind::Worker if name == "saves" => "Writes saves and snapshots".to_string(),
        Kind::Side if name == JIT_THREAD => "Turns game code into code for this device".to_string(),
        Kind::Worker if name.starts_with("session: ") => "The game you play together".to_string(),
        Kind::Worker if name.starts_with("watch: ") => "Their game, to watch".to_string(),
        Kind::Worker | Kind::Background | Kind::Side => String::new(),
    }
}

pub fn helper_runs(tasks: &[&str]) -> String {
    let words: Vec<&str> = tasks.iter().map(|task| task_words(task)).collect();
    let text = match words.as_slice() {
        [] => return "Idle".to_string(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn task_words(task: &str) -> &str {
    match task {
        "3d" => "the 3D render",
        "2d lines" => "2D lines",
        "linked console" => "a linked console",
        "friend catch-up" => "a friend's game",
        "snapshot" => "snapshots",
        "state check" => "state checks",
        "link speed check" => "a link speed check",
        other => other,
    }
}

fn order(name: &str, kind: Kind) -> u8 {
    match kind {
        Kind::Worker if name == "console" => 0,
        Kind::Worker if name.starts_with("session: ") => 1,
        Kind::Worker if name.starts_with("watch: ") => 2,
        Kind::Worker => 4,
        Kind::Pool => 3,
        Kind::Side if name == JIT_THREAD => 3,
        Kind::Background | Kind::Side => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_say_what_they_run_in_plain_words() {
        assert_eq!(helper_runs(&[]), "Idle");
        assert_eq!(helper_runs(&["3d"]), "The 3D render");
        assert_eq!(
            helper_runs(&["2d lines", "3d"]),
            "2D lines and the 3D render"
        );
        assert_eq!(
            helper_runs(&["linked console", "snapshot", "state check"]),
            "A linked console, snapshots and state checks"
        );
    }
}
