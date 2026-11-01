pub fn without_tags(name: &str) -> &str {
    let mut rest = name.trim_end();
    while let Some(open) = match rest.chars().last() {
        Some(')') => rest.rfind('('),
        Some(']') => rest.rfind('['),
        _ => None,
    } {
        rest = rest[..open].trim_end();
    }
    rest
}
