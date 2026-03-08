const CHANGELOG: &str = include_str!("../../../../../CHANGELOG.md");

pub fn build() -> String {
    CHANGELOG.to_string()
}
