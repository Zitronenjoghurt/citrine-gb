use crate::media::recording::codec::LICENSE;

pub fn build() -> String {
    markdown(LICENSE)
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
}
