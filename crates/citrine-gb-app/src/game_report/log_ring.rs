#![expect(
    clippy::disallowed_types,
    reason = "the logger is called from every thread, so the kept lines sit behind one global lock"
)]

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

const KEPT_LINES: usize = 2000;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static STARTED: OnceLock<web_time::Instant> = OnceLock::new();

struct RingLogger {
    inner: Option<Box<dyn Log>>,
}

impl Log for RingLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Info
            || self
                .inner
                .as_ref()
                .is_some_and(|inner| inner.enabled(metadata))
    }

    fn log(&self, record: &Record<'_>) {
        if record.level() <= Level::Info {
            keep(record);
        }
        if let Some(inner) = &self.inner {
            inner.log(record);
        }
    }

    fn flush(&self) {
        if let Some(inner) = &self.inner {
            inner.flush();
        }
    }
}

fn keep(record: &Record<'_>) {
    let seconds = STARTED
        .get_or_init(web_time::Instant::now)
        .elapsed()
        .as_secs_f64();
    let line = format!(
        "{seconds:10.3} {:<5} {}: {}",
        record.level(),
        record.target(),
        record.args()
    );
    let Ok(mut lines) = LINES.lock() else {
        return;
    };
    if lines.len() == KEPT_LINES {
        lines.pop_front();
    }
    lines.push_back(line);
}

pub fn install(inner: Option<Box<dyn Log>>, level: LevelFilter) {
    STARTED.get_or_init(web_time::Instant::now);
    if log::set_boxed_logger(Box::new(RingLogger { inner })).is_ok() {
        log::set_max_level(level.max(LevelFilter::Info));
    }
}

pub(crate) fn text() -> String {
    let Ok(lines) = LINES.lock() else {
        return String::new();
    };
    let mut text = String::new();
    for line in lines.iter() {
        text.push_str(line);
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_lines_carry_their_level_and_target_and_stop_at_the_limit() {
        for index in 0..KEPT_LINES + 5 {
            keep(
                &Record::builder()
                    .level(Level::Warn)
                    .target("citrine")
                    .args(format_args!("line {index}"))
                    .build(),
            );
        }
        let text = text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), KEPT_LINES);
        assert!(lines.last().unwrap().ends_with("WARN  citrine: line 2004"));
    }
}
