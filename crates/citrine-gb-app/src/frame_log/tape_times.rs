use super::COLUMNS;

#[derive(Debug, Clone, PartialEq)]
pub struct TapeTimes {
    pub game: Vec<(u64, f32)>,
    pub gpu: Vec<(u64, f32)>,
    pub budget_ms: f32,
}

impl TapeTimes {
    pub fn read(csv: &str) -> Option<Self> {
        let mut rows = Rows::new(csv);
        let header = rows.next()?;
        let at = |name: &str| header.iter().position(|column| column == name);
        let tape_frame = at("tape_frame")?;
        let game_ms = at("game_ms")?;
        let game_frames = at("game_frames")?;
        let gpu_ms = at("gpu_ms")?;
        let gpu_frames = at("gpu_frames")?;
        let budget_ms = at("budget_ms")?;
        let mut times = TapeTimes {
            game: Vec::new(),
            gpu: Vec::new(),
            budget_ms: 0.0,
        };
        for row in rows {
            let number = |index: usize| row.get(index).and_then(|field| field.parse::<f64>().ok());
            let Some(frame) = row
                .get(tape_frame)
                .and_then(|field| field.parse::<u64>().ok())
            else {
                continue;
            };
            let frames = number(game_frames).unwrap_or(0.0);
            if frames < 1.0 {
                continue;
            }
            let start = frame.saturating_sub(frames as u64);
            for series in [&mut times.game, &mut times.gpu] {
                while series.last().is_some_and(|(x, _)| *x > start) {
                    series.pop();
                }
            }
            if let Some(game) = number(game_ms) {
                times.game.push((frame, (game / frames) as f32));
            }
            if number(gpu_frames).unwrap_or(0.0) >= 1.0
                && let Some(gpu) = number(gpu_ms)
            {
                times.gpu.push((frame, gpu as f32));
            }
            if let Some(budget) = number(budget_ms).filter(|budget| *budget > 0.0) {
                times.budget_ms = budget as f32;
            }
        }
        (!times.game.is_empty()).then_some(times)
    }

    pub fn top(&self) -> f32 {
        let highest = self
            .game
            .iter()
            .chain(&self.gpu)
            .map(|(_, ms)| *ms)
            .fold(0.0, f32::max);
        let budget = if self.budget_ms > 0.0 {
            self.budget_ms
        } else {
            highest.max(1.0)
        };
        (highest * 1.1).max(budget * 1.5).min(budget * 3.0)
    }

    pub fn game_at(&self, frame: u64) -> Option<f32> {
        nearest(&self.game, frame)
    }

    pub fn gpu_at(&self, frame: u64) -> Option<f32> {
        nearest(&self.gpu, frame)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cropped {
    pub csv: String,
    pub rows: usize,
    pub began_s: f64,
    pub length_s: f64,
}

pub fn crop(csv: &str, from: u64, to: u64) -> Option<Cropped> {
    let mut rows = Rows::new(csv);
    let header = rows.next()?;
    let at = |name: &str| header.iter().position(|column| column == name);
    let tape_frame = at("tape_frame")?;
    let game_frames = at("game_frames")?;
    let time_s = at("time_s")?;
    let mut kept: Vec<Spot> = Vec::new();
    let mut before = None;
    for row in rows {
        let whole = |index: usize| row.get(index).and_then(|field| field.parse::<u64>().ok());
        let Some(frame) = whole(tape_frame) else {
            continue;
        };
        let start = frame.saturating_sub(whole(game_frames).unwrap_or(0));
        let time = row
            .get(time_s)
            .and_then(|field| field.parse::<f64>().ok())
            .unwrap_or(0.0);
        while kept.last().is_some_and(|spot| spot.frame > start) {
            kept.pop();
        }
        kept.push(Spot {
            frame,
            start,
            before: before.unwrap_or(time),
            time,
            row,
        });
        before = Some(time);
    }
    let mut cropped = Cropped {
        csv: line(&header),
        rows: 0,
        began_s: 0.0,
        length_s: 0.0,
    };
    let mut first = None;
    for mut spot in kept {
        if spot.start < from || spot.frame > to {
            continue;
        }
        let began = *first.get_or_insert(spot.before);
        cropped.began_s = began;
        cropped.length_s = spot.time - began;
        if let Some(field) = spot.row.get_mut(time_s) {
            *field = format!("{:.3}", spot.time - began);
        }
        if let Some(field) = spot.row.get_mut(tape_frame) {
            *field = (spot.frame - from).to_string();
        }
        cropped.csv.push_str(&line(&spot.row));
        cropped.rows += 1;
    }
    (cropped.rows > 0).then_some(cropped)
}

struct Spot {
    frame: u64,
    start: u64,
    before: f64,
    time: f64,
    row: Vec<String>,
}

fn line(fields: &[String]) -> String {
    let mut line = fields
        .iter()
        .map(|field| super::quoted(field))
        .collect::<Vec<_>>()
        .join(",");
    line.push('\n');
    line
}

fn nearest(series: &[(u64, f32)], frame: u64) -> Option<f32> {
    let after = series.partition_point(|(x, _)| *x < frame);
    [after.checked_sub(1), Some(after)]
        .into_iter()
        .flatten()
        .filter_map(|index| series.get(index))
        .min_by_key(|(x, _)| x.abs_diff(frame))
        .map(|(_, ms)| *ms)
}

struct Rows<'a> {
    rest: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Rows<'a> {
    fn new(csv: &'a str) -> Self {
        Self {
            rest: csv.chars().peekable(),
        }
    }
}

impl Iterator for Rows<'_> {
    type Item = Vec<String>;

    fn next(&mut self) -> Option<Vec<String>> {
        let mut fields = Vec::with_capacity(COLUMNS.len());
        let mut field = String::new();
        let mut quoted = false;
        let mut any = false;
        while let Some(c) = self.rest.next() {
            any = true;
            match c {
                '"' if quoted => {
                    if self.rest.next_if_eq(&'"').is_some() {
                        field.push('"');
                    } else {
                        quoted = false;
                    }
                }
                '"' => quoted = true,
                ',' if !quoted => fields.push(std::mem::take(&mut field)),
                '\n' if !quoted => break,
                '\r' if !quoted => {}
                c => field.push(c),
            }
        }
        if !any {
            return None;
        }
        fields.push(field);
        Some(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv(rows: &[(&str, &str, &str, &str, &str)]) -> String {
        let mut csv = COLUMNS.join(",");
        csv.push('\n');
        for (tape_frame, game_ms, game_frames, gpu_ms, gpu_frames) in rows {
            let row: Vec<String> = COLUMNS
                .iter()
                .map(|column| match *column {
                    "tape_frame" => tape_frame.to_string(),
                    "game_ms" => game_ms.to_string(),
                    "game_frames" => game_frames.to_string(),
                    "gpu_ms" => gpu_ms.to_string(),
                    "gpu_frames" => gpu_frames.to_string(),
                    "budget_ms" => "16.7".to_string(),
                    "game" => "\"Tobu, \"\"the\"\" game\"".to_string(),
                    "events" => "\"paused\nresumed\"".to_string(),
                    _ => String::new(),
                })
                .collect();
            csv.push_str(&row.join(","));
            csv.push('\n');
        }
        csv
    }

    #[test]
    fn the_times_follow_the_tape_and_a_rewind_keeps_the_frames_after_it() {
        let times = TapeTimes::read(&csv(&[
            ("1", "4", "1", "2.5", "1"),
            ("3", "10", "2", "", "0"),
            ("", "8", "1", "", "0"),
            ("4", "6", "1", "", "0"),
            ("5", "40", "1", "", "0"),
            ("3", "7", "1", "", "0"),
            ("4", "9", "1", "", "0"),
            ("4", "1", "0", "", "0"),
        ]))
        .expect("the recording has frame times");
        assert_eq!(times.game, vec![(1, 4.0), (3, 7.0), (4, 9.0)]);
        assert_eq!(times.gpu, vec![(1, 2.5)]);
        assert!((times.budget_ms - 16.7).abs() < 0.001);
        assert_eq!(times.game_at(2), Some(4.0));
        assert_eq!(times.game_at(9), Some(9.0));
        assert_eq!(times.gpu_at(1), Some(2.5));
    }

    #[test]
    fn a_video_only_recording_has_no_tape_times() {
        assert_eq!(TapeTimes::read(&csv(&[("", "4", "1", "", "0")])), None);
        assert_eq!(TapeTimes::read(""), None);
        assert_eq!(TapeTimes::read("time_s,game_ms\n1,2\n"), None);
    }

    #[test]
    fn the_graph_top_leaves_room_over_the_budget_and_caps_spikes() {
        let mut times = TapeTimes::read(&csv(&[("1", "4", "1", "", "0")])).unwrap();
        assert!((times.top() - 16.7 * 1.5).abs() < 0.01);
        times.game.push((2, 400.0));
        assert!((times.top() - 16.7 * 3.0).abs() < 0.01);
    }

    #[test]
    fn a_crop_keeps_the_rows_inside_it_counted_from_its_start() {
        let csv = "time_s,game_ms,game_frames,game,tape_frame\n\
                   0.000,0,0,\"Tobu, the game\",0\n\
                   0.050,8,2,\"Tobu, the game\",2\n\
                   0.100,5,1,\"Tobu, the game\",3\n\
                   0.120,1,0,\"Tobu, the game\",\n\
                   0.150,12,2,\"Tobu, the game\",5\n\
                   0.200,6,1,\"Tobu, the game\",6\n";
        let cropped = crop(csv, 2, 5).expect("rows inside the crop");
        assert_eq!(
            cropped.csv,
            "time_s,game_ms,game_frames,game,tape_frame\n\
             0.050,5,1,\"Tobu, the game\",1\n\
             0.100,12,2,\"Tobu, the game\",3\n"
        );
        assert_eq!(cropped.rows, 2);
        assert!((cropped.began_s - 0.05).abs() < 1e-9);
        assert!((cropped.length_s - 0.1).abs() < 1e-9);
        assert_eq!(crop(csv, 7, 9), None);
        assert_eq!(crop("time_s,game_ms\n1,2\n", 0, 9), None);
    }

    #[test]
    fn a_crop_after_a_rewind_keeps_only_the_frames_played_after_it() {
        let csv = "time_s,game_ms,game_frames,tape_frame\n\
                   0.000,0,0,0\n\
                   0.050,8,2,2\n\
                   0.100,5,1,3\n\
                   0.150,12,2,5\n\
                   0.200,6,1,6\n\
                   0.300,7,1,3\n\
                   0.350,9,2,5\n";
        let cropped = crop(csv, 2, 5).expect("rows inside the crop");
        assert_eq!(
            cropped.csv,
            "time_s,game_ms,game_frames,tape_frame\n\
             0.100,7,1,1\n\
             0.150,9,2,3\n"
        );
        assert_eq!(cropped.rows, 2);
        assert!((cropped.began_s - 0.2).abs() < 1e-9);
        assert!((cropped.length_s - 0.15).abs() < 1e-9);
    }
}
