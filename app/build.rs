use quote::quote;
use std::collections::HashMap;
use std::path::Path;
use std::{env, fs};

#[derive(serde::Deserialize, Default)]
struct GameMeta {
    title: String,
    author: String,
    description: String,
    license: String,
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
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

    let mut games = Vec::new();
    for entry in fs::read_dir(homebrew_dir).unwrap() {
        let path = entry.unwrap().path();
        let ext = path.extension().unwrap().to_str().unwrap();

        if ext != "gb" && ext != "gbc" {
            continue;
        };

        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let meta = index.remove(&stem).unwrap();

        games.push((path, stem, meta));
    }

    games.sort_by(|a, b| a.2.title.cmp(&b.2.title));

    for (path, stem, meta) in games {
        let raw_data = fs::read(&path).unwrap();
        let compressed_data = zstd::stream::encode_all(raw_data.as_slice(), 22).unwrap();
        let compressed_filename = format!("{}.zst", stem);
        let compressed_path = Path::new(&out_dir).join(&compressed_filename);
        fs::write(&compressed_path, compressed_data).unwrap();

        let title = meta.title;
        let author = meta.author;
        let description = meta.description;
        let license = meta.license;
        let link_tokens = meta.links.iter().map(|link| quote! { #link });
        let tag_tokens = meta.tags.iter().map(|tag| quote! { #tag });

        game_tokens.push(quote! {
            HomebrewGame {
                id: #stem,
                title: #title,
                author: #author,
                description: #description,
                license: #license,
                links: &[ #(#link_tokens),* ],
                tags: &[ #(#tag_tokens),* ],
                data: include_bytes!(concat!(env!("OUT_DIR"), "/", #compressed_filename)),
            }
        });
    }

    let final_code = quote! {
        #[derive(Clone, Copy)]
        pub struct HomebrewGame {
            pub id: &'static str,
            pub title: &'static str,
            pub author: &'static str,
            pub description: &'static str,
            pub license: &'static str,
            pub links: &'static [&'static str],
            pub tags: &'static [&'static str],
            data: &'static [u8],
        }

        impl HomebrewGame {
            pub fn tag_str(&self) -> String {
                self.tags.join(", ")
            }

            pub fn data(&self) -> Vec<u8> {
                zstd::stream::decode_all(self.data).unwrap()
            }
        }

        pub const HOMEBREW_GAMES: &[HomebrewGame] = &[
            #(#game_tokens),*
        ];
    };

    fs::write(dest_path, final_code.to_string()).unwrap();
}
