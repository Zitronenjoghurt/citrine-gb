use quote::quote;
use std::io::Write;
use std::path::Path;
use std::{env, fs};

#[path = "src/game_name/tags.rs"]
mod tags;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    game_names(Path::new(&out_dir));
    ds_names(Path::new(&out_dir));
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

fn ds_names(out_dir: &Path) {
    let mut entries: Vec<([u8; 4], String)> = Vec::new();
    if env::var_os("CARGO_FEATURE_NDS").is_some() {
        let xml = without_comments(&fs::read_to_string("names/nds.xml").unwrap());
        for software in xml.split("<software ").skip(1) {
            let description = software
                .split_once("<description>")
                .and_then(|(_, rest)| rest.split_once("</description>"))
                .unwrap()
                .0;
            let serial = software
                .split_once("name=\"serial\" value=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .unwrap()
                .0;
            let name = tags::without_tags(&unescape(description)).to_string();
            entries.push((ds_code(serial), name));
        }
        let list = fs::read_to_string("names/nds-wikidata.tsv").unwrap();
        for line in list.lines().filter(|line| !line.starts_with('#')) {
            let (code, name) = line.split_once('\t').unwrap();
            let code: [u8; 4] = code.as_bytes().try_into().unwrap();
            entries.push((code, tags::without_tags(name).to_string()));
        }
    }
    let mut seen = std::collections::HashSet::new();
    entries.retain(|entry| seen.insert(entry.0));
    entries.sort();
    let tokens = entries.iter().map(|(code, name)| {
        let code = code.iter();
        quote! { ([ #(#code),* ], #name) }
    });
    let code = quote! {
        static DS_NAMES: &[([u8; 4], &str)] = &[ #(#tokens),* ];
    };
    fs::write(out_dir.join("ds_names.rs"), code.to_string()).unwrap();
}

fn ds_code(serial: &str) -> [u8; 4] {
    let rest = serial.strip_prefix("NTR").unwrap();
    let rest = rest.trim_start_matches([' ', '-']);
    let rest = rest.strip_prefix('P').unwrap();
    let rest = rest.trim_start_matches([' ', '-']);
    rest.as_bytes()[..4].try_into().unwrap()
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
