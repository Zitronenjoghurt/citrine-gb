pub fn fetched(file_name: &str, target_arch: &str) -> bool {
    file_name.ends_with(".gba.br") && target_arch == "wasm32"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_gba_games_on_the_web_are_fetched() {
        assert!(fetched("pentris.gba.br", "wasm32"));
        assert!(!fetched("pentris.gba.br", "x86_64"));
        assert!(!fetched("pentris.gba.br", "aarch64"));
        assert!(!fetched("snake.gb", "wasm32"));
        assert!(!fetched("ucity.gbc", "wasm32"));
    }
}
