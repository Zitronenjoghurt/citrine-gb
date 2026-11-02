pub const DISPLAY_NAME_MAX_CHARS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("the name is empty")]
    Empty,
    #[error("the name is longer than 24 characters")]
    TooLong,
    #[error("the name has a control character")]
    ControlChar,
}

pub fn display_name(input: &str) -> Result<String, NameError> {
    let name = input.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.chars().any(char::is_control) {
        return Err(NameError::ControlChar);
    }
    if name.chars().count() > DISPLAY_NAME_MAX_CHARS {
        return Err(NameError::TooLong);
    }
    Ok(name.to_string())
}

pub fn device_name(input: &str) -> String {
    let name: String = input
        .chars()
        .filter(|c| !c.is_control())
        .take(crate::MAX_DEVICE_NAME_CHARS)
        .collect();
    match name.trim() {
        "" => "Device".into(),
        name => name.to_string(),
    }
}

pub fn is_phone(device_name: &str) -> bool {
    ["phone", "Android", "iPhone", "iPad"]
        .iter()
        .any(|word| device_name.contains(word))
}

pub fn initials(name: &str) -> String {
    let mut words = name.split_whitespace();
    let first = words.next().and_then(|w| w.chars().next());
    let second = words.next().and_then(|w| w.chars().next());
    match (first, second) {
        (Some(a), Some(b)) => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), None) => a.to_uppercase().collect(),
        _ => "?".into(),
    }
}

pub fn avatar_hues(seed: u64) -> (u16, u16) {
    let mixed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let hue = mixed % 360;
    let shift = 30 + (mixed >> 20) % 50;
    (hue as u16, ((hue + shift) % 360) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_are_cleaned_and_never_empty() {
        assert_eq!(device_name("  Firefox on Mac "), "Firefox on Mac");
        assert_eq!(device_name("Linux\u{7}PC"), "LinuxPC");
        assert_eq!(device_name(" \n"), "Device");
        assert_eq!(device_name(&"x".repeat(90)).chars().count(), 40);
    }

    #[test]
    fn phones_are_told_from_computers() {
        assert!(is_phone("Android phone"));
        assert!(is_phone("Chrome on Android"));
        assert!(is_phone("Safari on iPhone"));
        assert!(!is_phone("Linux PC"));
        assert!(!is_phone("Firefox on Mac"));
    }

    #[test]
    fn initials_from_names() {
        assert_eq!(initials("Clara"), "C");
        assert_eq!(initials("ana maria"), "AM");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("ßeta"), "SS");
    }

    #[test]
    fn avatar_hues_are_stable_and_spread() {
        assert_eq!(avatar_hues(42), avatar_hues(42));
        let (a, b) = avatar_hues(42);
        assert!(a < 360 && b < 360 && a != b);
        let hues: std::collections::BTreeSet<u16> = (0..20).map(|s| avatar_hues(s).0).collect();
        assert!(hues.len() > 10);
    }

    #[test]
    fn trims_and_accepts_any_script() {
        assert_eq!(display_name("  Clara ").unwrap(), "Clara");
        assert_eq!(display_name("Zitrone 🍋").unwrap(), "Zitrone 🍋");
        assert_eq!(display_name("さくら").unwrap(), "さくら");
        assert_eq!(display_name("Ünal-Öztürk").unwrap(), "Ünal-Öztürk");
        assert_eq!(display_name("a").unwrap(), "a");
    }

    #[test]
    fn counts_characters_not_bytes() {
        let longest = "ü".repeat(DISPLAY_NAME_MAX_CHARS);
        assert_eq!(display_name(&longest).unwrap(), longest);
        assert_eq!(
            display_name(&"ü".repeat(DISPLAY_NAME_MAX_CHARS + 1)),
            Err(NameError::TooLong)
        );
    }

    #[test]
    fn refuses_empty_and_control_characters() {
        assert_eq!(display_name(""), Err(NameError::Empty));
        assert_eq!(display_name(" \t\n "), Err(NameError::Empty));
        assert_eq!(display_name("a\nb"), Err(NameError::ControlChar));
        assert_eq!(display_name("a\u{7}b"), Err(NameError::ControlChar));
        assert_eq!(display_name("a\u{9b}b"), Err(NameError::ControlChar));
    }
}
