use quote::quote;
use sha1::Digest;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::{env, fs};

#[path = "src/game_name/tags.rs"]
mod tags;

#[path = "src/homebrew/storage.rs"]
mod storage;

#[derive(serde::Deserialize, Default)]
struct GameMeta {
    title: String,
    author: String,
    description: String,
    license: String,
    #[serde(default)]
    links: Vec<Link>,
    #[serde(default)]
    tags: Vec<String>,
    sha1: Option<String>,
    size: Option<usize>,
}

#[derive(serde::Deserialize)]
struct Link {
    url: String,
    kind: LinkKind,
}

#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum LinkKind {
    Itch,
    Github,
    Official,
}

impl LinkKind {
    fn as_tokens(&self) -> proc_macro2::TokenStream {
        match self {
            LinkKind::Itch => quote! { LinkKind::Itch },
            LinkKind::Github => quote! { LinkKind::Github },
            LinkKind::Official => quote! { LinkKind::Official },
        }
    }
}

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("./assets/icon-256x256.ico");
        res.compile().unwrap();
    }

    println!("cargo:rerun-if-changed=homebrew");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("homebrew.rs");
    let homebrew_dir = Path::new("./homebrew");
    let index_path = homebrew_dir.join("index.toml");

    let mut game_tokens = Vec::new();

    let mut index: HashMap<String, GameMeta> =
        toml::from_str(&fs::read_to_string(index_path).unwrap()).unwrap();

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let mut games = Vec::new();
    for entry in fs::read_dir(homebrew_dir).unwrap() {
        let path = entry.unwrap().path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };

        if ext != "gb" && ext != "gbc" {
            continue;
        };

        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let meta = index.remove(&stem).unwrap();

        games.push((path, stem, meta, false));
    }
    if env::var_os("CARGO_FEATURE_GBA").is_some() {
        for entry in fs::read_dir(homebrew_dir.join("gba")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            let Some(stem) = name.strip_suffix(".gba.br") else {
                continue;
            };
            let stem = stem.to_string();
            assert!(
                homebrew_dir
                    .join("gba")
                    .join(format!("{stem}.LICENSE.txt"))
                    .exists(),
                "{stem} has no licence file"
            );
            let meta = index.remove(&stem).unwrap();
            games.push((path, stem, meta, true));
        }
    }

    games.sort_by_cached_key(|game| (game.3, sort_key(&game.2.title), game.2.title.clone()));

    for (path, stem, meta, gba) in games {
        let stored = fs::read(&path).unwrap();
        let raw_data = if gba {
            let mut raw = Vec::new();
            std::io::Read::read_to_end(
                &mut brotli::Decompressor::new(stored.as_slice(), 4096),
                &mut raw,
            )
            .unwrap();
            raw
        } else {
            stored.clone()
        };
        let sha1: [u8; 20] = sha1::Sha1::digest(&raw_data).into();
        let sha1_hex: String = sha1.iter().map(|b| format!("{b:02x}")).collect();
        if gba {
            assert_eq!(meta.sha1.as_deref(), Some(sha1_hex.as_str()), "{stem} sha1");
            assert_eq!(meta.size, Some(raw_data.len()), "{stem} size");
            assert_eq!(raw_data[0xB2], 0x96, "{stem} is not a GBA game");
        }
        let rom_key: String = sha2::Sha256::digest(&raw_data)
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect();
        let cgb_mode = match raw_data[0x143] {
            _ if gba => quote! { citrine_gb::rom::header::RomCgbMode::None },
            0xC0 => quote! { citrine_gb::rom::header::RomCgbMode::CgbOnly },
            flag if flag & 0x80 != 0 => quote! { citrine_gb::rom::header::RomCgbMode::CgbAndGb },
            _ => quote! { citrine_gb::rom::header::RomCgbMode::None },
        };
        let console = if gba {
            quote! { ShelfConsole::GameBoyAdvance }
        } else {
            quote! { ShelfConsole::GameBoy }
        };
        let size = raw_data.len();
        let file_name = path.file_name().unwrap().to_str().unwrap();
        let rom = if storage::fetched(file_name, &target_arch) {
            let fetch_path = format!("homebrew/gba/{file_name}");
            let bytes = stored.len() as u64;
            quote! { ShelfRom::Fetch { path: #fetch_path, bytes: #bytes } }
        } else if gba {
            let source = format!("/homebrew/gba/{file_name}");
            quote! { ShelfRom::Embedded(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), #source))) }
        } else {
            let mut compressed_data = Vec::new();
            {
                let mut writer = brotli::CompressorWriter::new(&mut compressed_data, 4096, 11, 22);
                writer.write_all(&raw_data).unwrap();
            }
            let compressed_filename = format!("{}.br", stem);
            let compressed_path = Path::new(&out_dir).join(&compressed_filename);
            fs::write(&compressed_path, compressed_data).unwrap();
            quote! { ShelfRom::Embedded(include_bytes!(concat!(env!("OUT_DIR"), "/", #compressed_filename))) }
        };

        let title = meta.title;
        let author = meta.author;
        let description = meta.description;
        let license = meta.license;
        let link_tokens = meta.links.iter().map(|link| {
            let url = &link.url;
            let kind = link.kind.as_tokens();
            quote! { HomebrewLink { url: #url, kind: #kind } }
        });
        let tag_tokens = meta.tags.iter().map(|tag| quote! { #tag });
        let sha1 = sha1.iter();

        game_tokens.push(quote! {
            HomebrewGame {
                id: #stem,
                title: #title,
                author: #author,
                description: #description,
                license: #license,
                sha1: [ #(#sha1),* ],
                rom_key: #rom_key,
                cgb_mode: #cgb_mode,
                console: #console,
                size: #size,
                links: &[ #(#link_tokens),* ],
                tags: &[ #(#tag_tokens),* ],
                rom: #rom,
            }
        });
    }

    let final_code = quote! {
        #[derive(Clone, Copy)]
        pub enum LinkKind {
            Itch,
            Github,
            Official,
        }

        #[derive(Clone, Copy)]
        pub struct HomebrewLink {
            pub url: &'static str,
            pub kind: LinkKind,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum ShelfConsole {
            GameBoy,
            GameBoyAdvance,
        }

        #[derive(Clone, Copy)]
        pub enum ShelfRom {
            Embedded(&'static [u8]),
            Fetch { path: &'static str, bytes: u64 },
        }

        #[derive(Clone, Copy)]
        pub struct HomebrewGame {
            pub id: &'static str,
            pub title: &'static str,
            pub author: &'static str,
            pub description: &'static str,
            pub license: &'static str,
            pub sha1: [u8; 20],
            pub rom_key: &'static str,
            pub cgb_mode: citrine_gb::rom::header::RomCgbMode,
            pub console: ShelfConsole,
            pub size: usize,
            pub links: &'static [HomebrewLink],
            pub tags: &'static [&'static str],
            pub rom: ShelfRom,
        }

        impl HomebrewGame {
            pub fn tag_str(&self) -> String {
                self.tags.join(", ")
            }

            pub fn embedded(&self) -> Option<Vec<u8>> {
                let ShelfRom::Embedded(data) = self.rom else {
                    return None;
                };
                let mut decompressed = Vec::new();
                let mut decompressor = brotli::Decompressor::new(data, 4096);
                std::io::Read::read_to_end(&mut decompressor, &mut decompressed).expect("Failed to decompress ROM data");
                Some(decompressed)
            }
        }

        pub static HOMEBREW_GAMES: &[HomebrewGame] = &[
            #(#game_tokens),*
        ];
    };

    fs::write(dest_path, final_code.to_string()).unwrap();

    game_names(Path::new(&out_dir));
    emoji(Path::new(&out_dir));
}

fn game_names(out_dir: &Path) {
    println!("cargo:rerun-if-changed=names");
    let mut entries: Vec<([u8; 8], String)> = Vec::new();
    for list in ["names/gameboy.xml", "names/gbcolor.xml", "names/gba.xml"] {
        let xml = without_comments(&fs::read_to_string(list).unwrap());
        for software in xml.split("<software ").skip(1) {
            let description = software
                .split_once("<description>")
                .and_then(|(_, rest)| rest.split_once("</description>"))
                .unwrap()
                .0;
            let name = tags::without_tags(&unescape(description)).to_string();
            for rom in software.split(" sha1=\"").skip(1) {
                let mut key = [0; 8];
                for (i, byte) in key.iter_mut().enumerate() {
                    *byte = u8::from_str_radix(&rom[i * 2..i * 2 + 2], 16).unwrap();
                }
                entries.push((key, name.clone()));
            }
        }
    }
    entries.sort();
    entries.dedup_by_key(|entry| entry.0);

    let mut titles: Vec<&str> = entries.iter().map(|entry| entry.1.as_str()).collect();
    titles.sort();
    titles.dedup();
    assert!(titles.len() <= u16::MAX as usize);

    let mut table = (entries.len() as u32).to_le_bytes().to_vec();
    for (key, _) in &entries {
        table.extend_from_slice(key);
    }
    for (_, name) in &entries {
        let index = titles.binary_search(&name.as_str()).unwrap() as u16;
        table.extend_from_slice(&index.to_le_bytes());
    }
    table.extend_from_slice(titles.join("\n").as_bytes());

    let mut compressed = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 11, 22);
        writer.write_all(&table).unwrap();
    }
    fs::write(out_dir.join("game_names.br"), compressed).unwrap();
}

fn emoji(out_dir: &Path) {
    println!("cargo:rerun-if-changed=assets/emoji");
    let mut files: Vec<(String, String)> = fs::read_dir("assets/emoji")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "svg"))
        .map(|path| {
            let key = path.file_stem().unwrap().to_str().unwrap().to_string();
            let svg = fs::read_to_string(&path).unwrap();
            let svg = match (svg.split_once("<title>"), svg.split_once("</title>")) {
                (Some((before, _)), Some((_, after))) => format!("{before}{after}"),
                _ => svg,
            };
            (key, svg)
        })
        .collect();
    files.sort();
    let mut bundle = String::new();
    for (key, svg) in &files {
        bundle.push_str(key);
        bundle.push('\n');
        bundle.push_str(svg.trim());
        bundle.push('\n');
    }
    let mut compressed = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 11, 22);
        writer.write_all(bundle.as_bytes()).unwrap();
    }
    fs::write(out_dir.join("emoji.br"), compressed).unwrap();
}

fn without_comments(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some((before, after)) = rest.split_once("<!--") {
        out.push_str(before);
        rest = after.split_once("-->").map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn sort_key(title: &str) -> String {
    title
        .chars()
        .map(|c| {
            if matches!(c, '\u{b5}' | '\u{3bc}') {
                'm'
            } else {
                c
            }
        })
        .flat_map(char::to_lowercase)
        .collect()
}
