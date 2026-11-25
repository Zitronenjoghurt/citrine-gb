#[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
use crate::media::recording::codec::LICENSE;

const EMOJI_CREDIT: &str = "### Emoji\nThe emoji are by RhosGFX, dedicated to the public domain under Creative Commons CC0 1.0: https://creativecommons.org/publicdomain/zero/1.0/\n";

const EMOJI_FONT: &str = include_str!("../../../../assets/fonts/LICENSE-noto-emoji.txt");

const LIBRARIES: &str = include_str!("../../../../assets/licences.md");

const CITRINE: &str = include_str!("../../../../../../NOTICE.md");

const GPL: &str = include_str!("../../../../../../LICENSE");

const AGPL: &str = include_str!("../../../../../citrine-gb-nds-net/LICENSE-altwfc.txt");

const MULTIPLY: &str = include_str!(
    "../../../../../citrine-gb/src/gba/cpu/arm/multiply/LICENSE-zaydlang-multiply.txt"
);

struct Notice {
    title: &'static str,
    intro: &'static str,
    texts: &'static [&'static str],
}

const PROJECTS: &[Notice] = &[
    Notice {
        title: "SameBoy",
        intro: "Parts of the Game Boy sound, screen and start-up memory code follow SameBoy, used under the MIT license:",
        texts: &[include_str!(
            "../../../../../citrine-gb/src/gb/LICENSE-sameboy.txt"
        )],
    },
    Notice {
        title: "nds-interp",
        intro: "The DS 3D renderer's polygon edge code follows nds-interp, used under the MIT license:",
        texts: &[include_str!(
            "../../../../../citrine-gb/src/nds/render/LICENSE-nds-interp.txt"
        )],
    },
    Notice {
        title: "xBR",
        intro: "The DS 3D texture filter xBR is a port of Hyllian's xBR-lv2 shader, used under the MIT license:",
        texts: &[include_str!(
            "../../../../../citrine-gb-nds-gpu/src/LICENSE-xbr.txt"
        )],
    },
    Notice {
        title: "FreeBIOS",
        intro: "DS games start with FreeBIOS, used under the BSD 2-clause license:",
        texts: &[include_str!(
            "../../../../../citrine-gb/assets/nds-bios/LICENSE"
        )],
    },
    Notice {
        title: "Cult-of-GBA BIOS",
        intro: "GBA games start with the Cult-of-GBA BIOS, used under the MIT license:",
        texts: &[include_str!(
            "../../../../../citrine-gb/assets/gba-bios/LICENSE"
        )],
    },
    Notice {
        title: "DS save types",
        intro: "The list of DS save chips comes from SkyEmu. It is adapted from melonDS's game list, which the melonDS team released under the MIT license:",
        texts: &[include_str!(
            "../../../../../citrine-gb/assets/nds-saves/LICENSE"
        )],
    },
    Notice {
        title: "mGBA",
        intro: "Some cheat code decoders follow mGBA. Their source is at https://github.com/Zitronenjoghurt/citrine-gb, and the Mozilla Public License 2.0 is under Libraries.",
        texts: &[include_str!(
            "../../../../../citrine-gb/src/cheats/LICENSE-mgba.txt"
        )],
    },
    Notice {
        title: "ares",
        intro: "Game Boy Color colours are mixed with the formula from ares, used under the ISC license:",
        texts: &[include_str!(
            "../../../../../citrine-gb/src/gb/ppu/types/LICENSE-ares.txt"
        )],
    },
    Notice {
        title: "GBAHawk",
        intro: "The GBA sprite fetch timing follows GBAHawk by the BizHawk team, used under the MIT license:",
        texts: &[include_str!(
            "../../../../../citrine-gb/src/gba/ppu/LICENSE-gbahawk.txt"
        )],
    },
    Notice {
        title: "PokéAPI",
        intro: "Pokémon names in Japanese, Korean, French, German, Italian and Spanish come from PokéAPI, used under the BSD 3-clause license:",
        texts: &[include_str!(
            "../../../../../citrine-gb-pkm/data/LICENSE-pokeapi.txt"
        )],
    },
    Notice {
        title: "Nintendo WFC server list",
        intro: "The DS online stand-ins' server list cipher follows the GameSpy SDK, used under the BSD 3-clause license:",
        texts: &[include_str!(
            "../../../../../citrine-gb-nds-net/src/list_cipher/LICENSE-gamespy-sdk.txt"
        )],
    },
];

const BUNDLED: &[Notice] = &[
    Notice {
        title: "SDL",
        intro: "The Windows app reads controllers through SDL 3.4.18, used under the zlib license:",
        texts: &[
            include_str!("../../../../assets/notices/sdl.txt"),
            "SDL contains HIDAPI, used under its BSD license:",
            include_str!("../../../../assets/notices/hidapi.txt"),
            "SDL contains math code from fdlibm:",
            include_str!("../../../../assets/notices/fdlibm.txt"),
            "SDL contains yuv2rgb, used under the BSD 3-clause license:",
            include_str!("../../../../assets/notices/yuv2rgb.txt"),
        ],
    },
    Notice {
        title: "ring",
        intro: "ring's C and assembly code comes from BoringSSL and OpenSSL, used under the Apache License 2.0 and the ISC license below:",
        texts: &[include_str!("../../../../assets/notices/ring.txt")],
    },
    Notice {
        title: "mozjpeg",
        intro: "Citrine decodes webcam frames with mozjpeg. This software is based in part on the work of the Independent JPEG Group.",
        texts: &[],
    },
    Notice {
        title: "Hack",
        intro: "The monospace font is Hack:",
        texts: &[include_str!("../../../../assets/notices/hack.txt")],
    },
    Notice {
        title: "Phosphor Icons",
        intro: "The icons are Phosphor Icons 2.1, used under the MIT license:",
        texts: &[include_str!("../../../../assets/notices/phosphor.txt")],
    },
    Notice {
        title: "Android libraries",
        intro: "The Android app contains these libraries:",
        texts: &[
            include_str!("../../../../assets/notices/android.md"),
            include_str!("../../../../assets/notices/protobuf.txt"),
        ],
    },
];

pub fn build() -> String {
    static PAGE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PAGE.get_or_init(page).clone()
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
fn codec() -> String {
    format!("{}\n", markdown(LICENSE))
}

#[cfg(not(all(not(target_arch = "wasm32"), not(feature = "openh264-source"))))]
fn codec() -> String {
    String::new()
}

fn notices(list: &[Notice]) -> String {
    let mut out = String::new();
    for notice in list {
        out.push_str(&format!("### {}\n{}\n\n", notice.title, notice.intro));
        for text in notice.texts {
            out.push_str(&markdown(text));
            out.push('\n');
        }
    }
    out
}

fn page() -> String {
    let page = format!(
        "{}\n{}{EMOJI_CREDIT}\nEmoji that have no picture use the Noto Emoji font by Google, under this license:\n\n{}",
        markdown(CITRINE),
        codec(),
        markdown(EMOJI_FONT)
    );
    let page = format!(
        "{page}\n### GBA multiply\nUsed under the zlib license:\n\n{}",
        markdown(MULTIPLY)
    );
    format!(
        "{page}\n{}## Libraries\nCitrine uses these libraries under their licences:\n\n{}{}\n## GNU General Public License\n{}\n## GNU Affero General Public License\n{}",
        notices(PROJECTS),
        notices(BUNDLED),
        markdown(LIBRARIES),
        markdown(GPL),
        markdown(AGPL)
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
        let framed = line
            .strip_prefix("---")
            .and_then(|rest| rest.strip_suffix("---"))
            .map(|title| title.trim_matches('-').trim())
            .filter(|title| !title.is_empty());
        if let Some(title) = framed {
            out.push_str("### ");
            out.push_str(title);
            out.push('\n');
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
    use std::path::{Path, PathBuf};

    const PORTED: &[(&str, &str)] = &[
        ("crates/citrine-gb/src/gb/apu.rs", "SameBoy"),
        (
            "crates/citrine-gb/src/gb/apu/channels/channel_4.rs",
            "SameBoy",
        ),
        (
            "crates/citrine-gb/src/gb/apu/components/frequency_sweep.rs",
            "SameBoy",
        ),
        (
            "crates/citrine-gb/src/gb/apu/components/square_wave.rs",
            "SameBoy",
        ),
        (
            "crates/citrine-gb/src/gb/apu/components/volume_envelope.rs",
            "SameBoy",
        ),
        ("crates/citrine-gb/src/gb/ppu.rs", "SameBoy"),
        ("crates/citrine-gb/src/gb/ppu/fetcher.rs", "SameBoy"),
        ("crates/citrine-gb/src/gb/ppu/fifo.rs", "SameBoy"),
        ("crates/citrine-gb/src/gb/ram_init.rs", "SameBoy"),
        ("crates/citrine-gb/src/nds/render/slope.rs", "nds-interp"),
        ("crates/citrine-gb-nds-gpu/src/raster.wgsl", "xBR"),
        ("crates/citrine-gb-nds-gpu/src/raster.wgsl", "nds-interp"),
        (
            "crates/citrine-gb/assets/nds-bios/freebios7.bin",
            "FreeBIOS",
        ),
        (
            "crates/citrine-gb/assets/nds-bios/freebios9.bin",
            "FreeBIOS",
        ),
        (
            "crates/citrine-gb/assets/gba-bios/cult-of-gba.bin",
            "Cult-of-GBA BIOS",
        ),
        (
            "crates/citrine-gb/assets/nds-saves/saves.bin",
            "DS save types",
        ),
        ("crates/citrine-gb/src/cheats/gba.rs", "mGBA"),
        ("crates/citrine-gb/src/cheats/gba/codebreaker.rs", "mGBA"),
        ("crates/citrine-gb/src/cheats/gba/gameshark.rs", "mGBA"),
        ("crates/citrine-gb/src/cheats/gba/parv3.rs", "mGBA"),
        ("crates/citrine-gb/src/cheats/gb.rs", "mGBA"),
        ("crates/citrine-gb/src/cheats/ops.rs", "mGBA"),
        ("crates/citrine-gb/src/gb/ppu/types/color.rs", "ares"),
        ("crates/citrine-gb/src/gba/ppu/fetch.rs", "GBAHawk"),
        ("crates/citrine-gb-pkm/data/species_names.tsv", "PokéAPI"),
        (
            "crates/citrine-gb-nds-net/src/list_cipher/mod.rs",
            "Nintendo WFC server list",
        ),
    ];

    const NOT_IN_THE_APP: &[&str] = &[
        "target",
        "citrine-gb-bench",
        "citrine-gb-lab",
        "citrine-gb-server",
        "citrine-gb-walker-lab",
        "melonds-sys",
        "mgba-sys",
        "sameboy-sys",
    ];

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn copyright_lines(text: &str) -> impl Iterator<Item = &str> {
        text.lines().map(str::trim).filter(|line| {
            line.starts_with("Copyright") && line.contains(|c: char| c.is_ascii_digit())
        })
    }

    fn licence_files(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                let skip = name.starts_with('.') || NOT_IN_THE_APP.contains(&name.as_str());
                if !skip {
                    licence_files(&path, found);
                }
            } else if name.starts_with("LICENSE") {
                found.push(path);
            }
        }
    }

    #[test]
    fn every_ported_or_vendored_source_has_its_notice() {
        let page = build();
        for (source, title) in PORTED {
            assert!(
                repo().join(source).is_file(),
                "{source} moved: update PORTED"
            );
            assert!(
                page.contains(&format!("\n### {title}\n")),
                "{source} has no {title} notice"
            );
        }
    }

    #[test]
    fn every_licence_file_in_the_app_crates_is_on_the_page() {
        let page = build();
        let mut files = Vec::new();
        licence_files(&repo().join("crates"), &mut files);
        assert!(files.len() >= 9, "found only {files:?}");
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            for line in copyright_lines(&text) {
                assert!(
                    page.contains(line),
                    "{} is not on the page: {line}",
                    file.display()
                );
            }
        }
    }

    #[test]
    fn every_bundled_binary_sits_beside_its_licence() {
        for entry in std::fs::read_dir(repo().join("crates/citrine-gb/assets")).unwrap() {
            let dir = entry.unwrap().path();
            assert!(
                dir.join("LICENSE").is_file(),
                "{} has no LICENSE",
                dir.display()
            );
        }
    }

    #[test]
    fn notices_the_generator_already_lists_are_not_added_twice() {
        let libraries = markdown(LIBRARIES);
        for notice in PROJECTS.iter().chain(BUNDLED) {
            assert!(!libraries.contains(&format!("### {}\n", notice.title)));
            for text in notice.texts {
                for line in copyright_lines(text) {
                    assert!(!libraries.contains(line), "{} twice: {line}", notice.title);
                }
            }
        }
    }

    #[test]
    fn the_android_list_names_every_direct_android_dependency() {
        let gradle = std::fs::read_to_string(repo().join("android/app/build.gradle.kts")).unwrap();
        let android = include_str!("../../../../assets/notices/android.md");
        let mut named = 0;
        for line in gradle.lines().map(str::trim) {
            let Some(rest) = line.strip_prefix("implementation(\"") else {
                continue;
            };
            let coordinate = rest.trim_end_matches("\")");
            let mut parts = coordinate.split(':');
            let (group, artifact, version) = (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            );
            assert!(
                android.contains(&format!("- {group}: "))
                    && android.contains(&format!("{artifact} {version}")),
                "{coordinate} is not in assets/notices/android.md: run scripts/licences.sh"
            );
            named += 1;
        }
        assert!(named >= 6);
    }

    #[test]
    fn notices_that_point_to_a_licence_text_find_it_on_the_page() {
        let page = build();
        assert!(page.contains("### Mozilla Public License 2.0\n"));
        assert!(page.contains("### ISC License\n"));
        assert!(page.contains("### Apache License 2.0\n"));
        assert!(
            page.contains(
                "This software is based in part on the work of the Independent JPEG Group."
            )
        );
        assert!(
            page.contains("This Source Code Form is subject to the terms of the Mozilla Public")
        );
    }

    #[cfg(not(feature = "openh264-source"))]
    #[test]
    fn the_cisco_license_reads_as_headed_text() {
        let page = build();
        assert!(page.contains(crate::media::recording::CODEC_NOTICE));
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

    #[test]
    fn the_libraries_and_their_licences_are_on_the_page() {
        let page = build();
        assert!(page.contains("## Libraries\n"));
        assert!(page.contains("rcheevos-sys 0.1.0: Copyright (c) 2018 RetroAchievements.org"));
        assert!(page.contains("dimpl 0.9.0"));
        assert!(page.contains("### Apache License 2.0\n"));
        assert!(page.contains("### LLVM Exceptions to the Apache 2.0 License\n"));
        assert!(!markdown(LIBRARIES).contains("citrine-gb"));
        assert!(!page.contains("&quot;"));
    }

    #[test]
    fn the_zaydlang_multiply_license_is_on_the_page() {
        let page = build();
        assert!(page.contains("### GBA multiply\n"));
        assert!(page.contains("Copyright (c) 2024 zaydlang"));
        assert!(page.contains("1. The origin of this software must not be misrepresented"));
        assert!(page.contains("3. This notice may not be removed or altered"));
    }

    #[test]
    fn the_gamespy_sdk_license_is_on_the_page() {
        let page = build();
        assert!(page.contains("### Nintendo WFC server list\n"));
        assert!(page.contains("Copyright (c) 2011, IGN Entertainment"));
        assert!(page.contains("Neither the name of IGN nor the names of its contributors"));
    }
}
