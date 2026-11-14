pub fn fetched(file_name: &str, target_arch: &str) -> bool {
    (file_name.ends_with(".gba.br") || file_name.ends_with(".nds.br")) && target_arch == "wasm32"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_gba_and_ds_games_on_the_web_are_fetched() {
        assert!(fetched("pentris.gba.br", "wasm32"));
        assert!(fetched("pong_nds.nds.br", "wasm32"));
        assert!(!fetched("pong_nds.nds.br", "x86_64"));
        assert!(!fetched("pentris.gba.br", "x86_64"));
        assert!(!fetched("pentris.gba.br", "aarch64"));
        assert!(!fetched("snake.gb", "wasm32"));
        assert!(!fetched("ucity.gbc", "wasm32"));
    }
}
