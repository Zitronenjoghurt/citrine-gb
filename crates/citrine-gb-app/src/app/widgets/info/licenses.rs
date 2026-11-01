use crate::media::recording::codec::LICENSE;

const EMOJI_CREDIT: &str = "### Emoji\nThe emoji are by RhosGFX, dedicated to the public domain under Creative Commons CC0 1.0: https://creativecommons.org/publicdomain/zero/1.0/\n";

const EMOJI_FONT: &str = include_str!("../../../../assets/fonts/LICENSE-noto-emoji.txt");

pub fn build() -> String {
    format!(
        "{}\n{EMOJI_CREDIT}\nEmoji that have no picture use the Noto Emoji font by Google, under this license:\n\n{}",
        markdown(LICENSE),
        markdown(EMOJI_FONT)
    )
}

fn markdown(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let rule = |line: &str| !line.is_empty() && line.chars().all(|c| c == '-');
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if rule(line) {
            continue;
        }
        let between_rules =
            i > 0 && rule(lines[i - 1]) && lines.get(i + 1).is_some_and(|l| rule(l));
        if between_rules {
            out.push_str("### ");
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::recording::CODEC_NOTICE;

    #[test]
    fn the_cisco_license_reads_as_headed_text() {
        let page = build();
        assert!(page.contains(CODEC_NOTICE));
        assert!(page.contains("### BSD License\n"));
        assert!(page.contains("### AVC/H.264 Patent Portfolio License Conditions\n"));
        assert!(!page.lines().any(|line| line.starts_with("---")));
        assert!(!page.lines().any(|line| line.starts_with("    ")));
    }

    #[test]
    fn the_emoji_license_is_on_the_page() {
        let page = build();
        assert!(page.contains("### Emoji\nThe emoji are by RhosGFX"));
        assert!(page.contains("CC0 1.0"));
        assert!(!page.contains("Fluent"));
        assert!(page.contains("Noto Emoji"));
        assert!(page.contains("SIL Open Font License, Version 1.1"));
    }
}
