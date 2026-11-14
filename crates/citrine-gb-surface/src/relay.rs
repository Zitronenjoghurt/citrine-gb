use crate::pairing::Code;
use serde::{Deserialize, Serialize};

pub const PATH: &str = "/screen";
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
pub const BYTES_PER_SEC: f64 = 2.0 * 1024.0 * 1024.0;
pub const DOORS_PER_ADDRESS: usize = 4;
pub const WRONG_CODES_PER_MINUTE: u32 = 5;
pub const ACCOUNT_DOORS_PER_ADDRESS: usize = 16;
pub const LOOK_KEYS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DoorKey(pub [u8; 16]);

impl DoorKey {
    pub fn hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 32 || !text.is_ascii() {
            return None;
        }
        let mut bytes = [0; 16];
        for (at, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl std::fmt::Debug for DoorKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DoorKey(..)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Control {
    Open {
        code: Code,
        password: Option<String>,
    },
    Account {
        key: DoorKey,
        password: Option<String>,
    },
    Opened,
    Taken,
    Join(Code),
    Dial(DoorKey),
    Look(Vec<DoorKey>),
    Here(Vec<DoorKey>),
    Joined,
    NoCode,
    Busy,
    Gone,
    Limit,
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open { code, password } => f
                .debug_struct("Open")
                .field("code", code)
                .field("password", &password.as_ref().map(|_| ".."))
                .finish(),
            Self::Account { .. } => f.write_str("Account(..)"),
            Self::Dial(_) => f.write_str("Dial(..)"),
            Self::Look(keys) => write!(f, "Look({})", keys.len()),
            Self::Here(keys) => write!(f, "Here({})", keys.len()),
            other => f.write_str(&other.text()),
        }
    }
}

impl Control {
    pub fn text(&self) -> String {
        match self {
            Self::Open {
                code,
                password: None,
            } => format!("open {}", code.plain()),
            Self::Open {
                code,
                password: Some(password),
            } => format!("open {} {password}", code.plain()),
            Self::Account {
                key,
                password: None,
            } => format!("account {}", key.hex()),
            Self::Account {
                key,
                password: Some(password),
            } => format!("account {} {password}", key.hex()),
            Self::Join(code) => format!("join {}", code.plain()),
            Self::Dial(key) => format!("dial {}", key.hex()),
            Self::Look(keys) => keys_text("look", keys),
            Self::Here(keys) => keys_text("here", keys),
            Self::Opened => "opened".into(),
            Self::Taken => "taken".into(),
            Self::Joined => "joined".into(),
            Self::NoCode => "nocode".into(),
            Self::Busy => "busy".into(),
            Self::Gone => "gone".into(),
            Self::Limit => "limit".into(),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        if let Some(keys) = text.strip_prefix("look") {
            return parse_keys(keys).map(Self::Look);
        }
        if let Some(keys) = text.strip_prefix("here") {
            return parse_keys(keys).map(Self::Here);
        }
        let mut words = text.splitn(3, ' ');
        let head = words.next()?;
        let code = words.next().map(Code::parse);
        let rest = words.next();
        if head == "account" {
            let key = text.split(' ').nth(1).and_then(DoorKey::parse)?;
            return Some(Self::Account {
                key,
                password: rest.map(str::to_string),
            });
        }
        if head == "dial" {
            let key = text.split(' ').nth(1).and_then(DoorKey::parse);
            return key.filter(|_| rest.is_none()).map(Self::Dial);
        }
        if let ("open", Some(Some(code))) = (head, code) {
            return Some(Self::Open {
                code,
                password: rest.map(str::to_string),
            });
        }
        if rest.is_some() {
            return None;
        }
        Some(match (head, code) {
            ("join", Some(Some(code))) => Self::Join(code),
            ("opened", None) => Self::Opened,
            ("taken", None) => Self::Taken,
            ("joined", None) => Self::Joined,
            ("nocode", None) => Self::NoCode,
            ("busy", None) => Self::Busy,
            ("gone", None) => Self::Gone,
            ("limit", None) => Self::Limit,
            _ => return None,
        })
    }
}

fn keys_text(head: &str, keys: &[DoorKey]) -> String {
    let mut text = head.to_owned();
    for key in keys {
        text.push(' ');
        text.push_str(&key.hex());
    }
    text
}

fn parse_keys(rest: &str) -> Option<Vec<DoorKey>> {
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let keys = rest
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(DoorKey::parse)
        .collect::<Option<Vec<_>>>()?;
    (keys.len() <= LOOK_KEYS).then_some(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_round_trip_as_text() {
        let code = Code::parse("012345").unwrap();
        for control in [
            Control::Open {
                code,
                password: None,
            },
            Control::Open {
                code,
                password: Some("a long pass".into()),
            },
            Control::Join(code),
            Control::Account {
                key: DoorKey([7; 16]),
                password: None,
            },
            Control::Account {
                key: DoorKey([0xab; 16]),
                password: Some("a long pass".into()),
            },
            Control::Dial(DoorKey([1; 16])),
            Control::Look(vec![DoorKey([1; 16]), DoorKey([2; 16])]),
            Control::Look(Vec::new()),
            Control::Here(vec![DoorKey([2; 16])]),
            Control::Here(Vec::new()),
            Control::Opened,
            Control::Taken,
            Control::Joined,
            Control::NoCode,
            Control::Busy,
            Control::Gone,
            Control::Limit,
        ] {
            assert_eq!(Control::parse(&control.text()), Some(control.clone()));
        }
        let open = Control::Open {
            code,
            password: None,
        };
        assert_eq!(open.text(), "open 012345");
        assert_eq!(
            Control::Dial(DoorKey([0x0f; 16])).text(),
            "dial 0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f"
        );
    }

    #[test]
    fn a_look_carries_a_bounded_number_of_keys() {
        let many = vec![DoorKey([3; 16]); LOOK_KEYS + 1];
        assert_eq!(Control::parse(&Control::Look(many).text()), None);
    }

    #[test]
    fn junk_is_no_control() {
        for text in [
            "",
            "open",
            "open 12",
            "join 123456 x",
            "joined now",
            "hello",
            "account",
            "account 12",
            "dial 0102",
            "dial 01010101010101010101010101010101 x",
            "looking",
            "look zz",
        ] {
            assert_eq!(Control::parse(text), None, "{text}");
        }
    }
}
