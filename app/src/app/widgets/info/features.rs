const FEATURES: &str = include_str!("../../../../../FEATURES.md");

pub fn build() -> String {
    FEATURES.to_string()
}
