use crate::walker::Button;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Wait(f64),
    Press(Button),
    Hold(Button, f64),
    Walk(u32, f64),
    Frame(String),
    Ir(Vec<u8>),
    IrLog,
    Identify,
    Connect(Vec<(u8, u8)>),
    EepromReads,
    Trace(u16, u16),
    TraceDump,
    Ram(u16, u16),
    Count(u16),
    Counts,
}

fn button(s: &str) -> Result<Button, String> {
    match s {
        "left" => Ok(Button::Left),
        "enter" => Ok(Button::Enter),
        "right" => Ok(Button::Right),
        _ => Err(format!("unknown button {s}")),
    }
}

fn num<T: std::str::FromStr>(s: Option<&str>, what: &str) -> Result<T, String> {
    s.ok_or(format!("missing {what}"))?
        .parse()
        .map_err(|_| format!("bad {what}"))
}

fn hex16(s: Option<&str>, what: &str) -> Result<u16, String> {
    let s = s.ok_or(format!("missing {what}"))?;
    u16::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|_| format!("bad {what}"))
}

pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let cmd = words.next().unwrap_or("");
        let step = match cmd {
            "wait" => num(words.next(), "seconds").map(Step::Wait),
            "press" => button(words.next().unwrap_or("")).map(Step::Press),
            "hold" => button(words.next().unwrap_or(""))
                .and_then(|b| Ok(Step::Hold(b, num(words.next(), "seconds")?))),
            "walk" => num(words.next(), "steps")
                .map(|s| Step::Walk(s, words.next().and_then(|r| r.parse().ok()).unwrap_or(2.0))),
            "frame" => words
                .next()
                .map(|s| Step::Frame(s.to_string()))
                .ok_or("missing name".to_string()),
            "ir" => words
                .map(|w| u8::from_str_radix(w, 16).map_err(|_| format!("bad byte {w}")))
                .collect::<Result<Vec<_>, _>>()
                .map(Step::Ir),
            "ir-log" => Ok(Step::IrLog),
            "identify" => Ok(Step::Identify),
            "connect" => words
                .map(|w| {
                    let (c, e) = w.split_once(':').unwrap_or((w, "01"));
                    match (u8::from_str_radix(c, 16), u8::from_str_radix(e, 16)) {
                        (Ok(c), Ok(e)) => Ok((c, e)),
                        _ => Err(format!("bad command {w}")),
                    }
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Step::Connect),
            "eeprom-reads" => Ok(Step::EepromReads),
            "trace" => hex16(words.next(), "from")
                .and_then(|a| Ok(Step::Trace(a, hex16(words.next(), "to")?))),
            "trace-dump" => Ok(Step::TraceDump),
            "ram" => hex16(words.next(), "address")
                .and_then(|a| Ok(Step::Ram(a, hex16(words.next(), "length")?))),
            "count" => hex16(words.next(), "address").map(Step::Count),
            "counts" => Ok(Step::Counts),
            _ => Err(format!("unknown command {cmd}")),
        };
        steps.push(step.map_err(|e| format!("line {}: {e}", n + 1))?);
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_command_and_skips_comments() {
        let text = "wait 1.5\npress enter # select\n\nhold left 2\nwalk 100\nwalk 50 1.5\nframe home\nir 56 aa\nir-log\nidentify\nconnect 32 66:02\neeprom-reads\nram f780 10\ncount 7348\ncounts\n";
        assert_eq!(
            parse(text).unwrap(),
            vec![
                Step::Wait(1.5),
                Step::Press(Button::Enter),
                Step::Hold(Button::Left, 2.0),
                Step::Walk(100, 2.0),
                Step::Walk(50, 1.5),
                Step::Frame("home".into()),
                Step::Ir(vec![0x56, 0xAA]),
                Step::IrLog,
                Step::Identify,
                Step::Connect(vec![(0x32, 0x01), (0x66, 0x02)]),
                Step::EepromReads,
                Step::Ram(0xF780, 0x10),
                Step::Count(0x7348),
                Step::Counts,
            ]
        );
    }

    #[test]
    fn errors_name_the_line() {
        assert_eq!(
            parse("wait 1\npress up").unwrap_err(),
            "line 2: unknown button up"
        );
    }
}
