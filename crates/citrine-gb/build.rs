use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(jit)");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let family = std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let native = matches!(arch.as_str(), "x86_64" | "aarch64");
    let unix = family.split(',').any(|f| f == "unix") && os != "ios";
    let windows = os == "windows" && arch == "x86_64";
    let allowed = unix || windows;
    let jit = std::env::var_os("CARGO_FEATURE_JIT").is_some();
    if jit && native && allowed {
        println!("cargo::rustc-cfg=jit");
    }
    let build = if jit { build_hash() } else { 0 };
    println!("cargo::rustc-env=CITRINE_JIT_BUILD={build:016x}");
    println!("cargo::rerun-if-changed=src");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=../../Cargo.lock");
}

fn build_hash() -> u64 {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let mut files = Vec::new();
    collect(&root.join("src"), &mut files);
    files.sort();
    files.push(root.join("../../Cargo.lock"));
    let mut text = Vec::new();
    for file in &files {
        text.extend_from_slice(
            file.strip_prefix(&root)
                .unwrap_or(file)
                .to_string_lossy()
                .as_bytes(),
        );
        text.extend_from_slice(&std::fs::read(file).unwrap_or_default());
    }
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = std::process::Command::new(rustc)
        .arg("-vV")
        .output()
        .map(|out| out.stdout)
        .unwrap_or_default();
    text.extend_from_slice(&version);
    let mut vars: Vec<(String, String)> = std::env::vars()
        .filter(|(name, _)| {
            name.starts_with("CARGO_FEATURE_")
                || matches!(
                    name.as_str(),
                    "TARGET" | "PROFILE" | "OPT_LEVEL" | "DEBUG" | "CARGO_ENCODED_RUSTFLAGS"
                )
        })
        .collect();
    vars.sort();
    for (name, value) in vars {
        text.extend_from_slice(format!("{name}={value}\n").as_bytes());
    }
    digest(&text)
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn digest(bytes: &[u8]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut hash = 0xCBF2_9CE4_8422_2325u64 ^ bytes.len() as u64;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(K).rotate_left(29);
    }
    hash
}
