use citrine_gb::console::{Keys, Touch};
use std::fmt::Write;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameInput {
    pub keys: Keys,
    pub touch: Option<Touch>,
    pub reset: bool,
}

impl FrameInput {
    pub fn line(&self) -> String {
        let mut line = format!("{:04x}", self.keys.bits());
        match self.touch {
            Some(touch) => {
                let _ = write!(line, " {},{}", touch.x, touch.y);
            }
            None => line.push_str(" -"),
        }
        if self.reset {
            line.push_str(" reset");
        }
        line
    }

    pub fn parse(line: &str) -> Option<Self> {
        let mut parts = line.split_whitespace();
        let keys = Keys::from_bits_truncate(u16::from_str_radix(parts.next()?, 16).ok()?);
        let touch = match parts.next()? {
            "-" => None,
            point => {
                let (x, y) = point.split_once(',')?;
                Some(Touch {
                    x: x.parse().ok()?,
                    y: y.parse().ok()?,
                })
            }
        };
        let reset = match parts.next() {
            None => false,
            Some("reset") => true,
            Some(_) => return None,
        };
        Some(Self { keys, touch, reset })
    }
}

pub(crate) fn encode(inputs: &[FrameInput]) -> String {
    let mut text = String::with_capacity(inputs.len() * 8);
    for input in inputs {
        text.push_str(&input.line());
        text.push('\n');
    }
    text
}

pub(crate) fn decode(text: &str) -> Option<Vec<FrameInput>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(FrameInput::parse)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_round_trip_through_their_text() {
        let inputs = vec![
            FrameInput::default(),
            FrameInput {
                keys: Keys::A | Keys::START,
                touch: Some(Touch { x: 128, y: 191 }),
                reset: false,
            },
            FrameInput {
                keys: Keys::empty(),
                touch: None,
                reset: true,
            },
        ];
        let text = encode(&inputs);
        assert_eq!(text.lines().nth(2), Some("0000 - reset"));
        assert_eq!(decode(&text), Some(inputs));
    }

    #[test]
    fn a_broken_line_fails_the_whole_list() {
        assert_eq!(decode("0001 -\nzz -\n"), None);
    }
}
