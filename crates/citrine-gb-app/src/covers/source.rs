use super::names::System;

const NAMES_ROOT: &str =
    "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro";
const ART_ROOT: &str = "https://raw.githubusercontent.com/libretro-thumbnails";
const LINK_MAX: usize = 255;

pub fn names_url(system: System) -> String {
    format!("{NAMES_ROOT}/{}.dat", encode(system.title()))
}

pub fn art_url(system: System, file: &str) -> String {
    let repo = system.title().replace(' ', "_");
    format!("{ART_ROOT}/{repo}/master/Named_Boxarts/{}", encode(file))
}

pub fn art_files(name: &str) -> Vec<String> {
    let mut files = vec![file_name(name)];
    let tags = name
        .match_indices(" (")
        .map(|(at, _)| at)
        .collect::<Vec<_>>();
    if let [_, second, ..] = tags[..] {
        files.push(file_name(&name[..second]));
    }
    files
}

pub fn file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| match c {
            '&' | '*' | '/' | ':' | '`' | '<' | '>' | '?' | '\\' | '|' | '"' => '_',
            c => c,
        })
        .collect();
    format!("{safe}.png")
}

pub fn link_target(body: &[u8]) -> Option<&str> {
    if body.len() > LINK_MAX || body.starts_with(b"\x89PNG") {
        return None;
    }
    let target = std::str::from_utf8(body).ok()?.trim();
    (target.ends_with(".png") && !target.contains('/') && !target.contains('\n')).then_some(target)
}

pub(crate) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_becomes_the_thumbnail_file_libretro_uses() {
        assert_eq!(
            file_name("Q*Bert's Qubes (USA)"),
            "Q_Bert's Qubes (USA).png"
        );
        assert_eq!(
            file_name("Yu-Gi-Oh! - World Championship 2008 (USA) (En,Fr,De,Es,It)"),
            "Yu-Gi-Oh! - World Championship 2008 (USA) (En,Fr,De,Es,It).png"
        );
        assert_eq!(
            file_name(r#"a&b/c:d`e<f>g?h\i|j"k"#),
            "a_b_c_d_e_f_g_h_i_j_k.png"
        );
    }

    #[test]
    fn a_name_with_extra_tags_also_tries_title_and_region() {
        assert_eq!(
            art_files("Tetris (World) (Rev 1)"),
            ["Tetris (World) (Rev 1).png", "Tetris (World).png"]
        );
        assert_eq!(art_files("Tetris (World)"), ["Tetris (World).png"]);
    }

    #[test]
    fn the_addresses_point_at_the_libretro_repositories() {
        assert_eq!(
            names_url(System::Gbc),
            "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Color.dat"
        );
        assert_eq!(
            art_url(System::Nds, "Pokemon - Platinum Version (USA).png"),
            "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Nintendo_DS/master/Named_Boxarts/Pokemon%20-%20Platinum%20Version%20%28USA%29.png"
        );
        assert_eq!(
            art_url(System::Gb, "Pokémon & co.png"),
            "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Game_Boy/master/Named_Boxarts/Pok%C3%A9mon%20%26%20co.png"
        );
    }

    #[test]
    fn a_linked_file_names_its_target() {
        assert_eq!(
            link_target(b"Pokemon - Crystal Version (USA).png"),
            Some("Pokemon - Crystal Version (USA).png")
        );
        assert_eq!(link_target(b"\x89PNG\r\n\x1a\n"), None);
        assert_eq!(link_target(b"../../etc/passwd.png"), None);
        assert_eq!(link_target(b"404: Not Found"), None);
        assert_eq!(link_target(&[b'a'; 300]), None);
    }
}
