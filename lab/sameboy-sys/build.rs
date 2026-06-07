use std::path::Path;

/// Files only needed for the interactive debugger / cheat search, which we disable.
const SKIP_FILES: &[&str] = &[
    "debugger.c",
    "sm83_disassembler.c",
    "symbol_hash.c",
    "cheat_search.c",
];

fn main() {
    let core_path = Path::new("../SameBoy/Core");
    if !core_path.is_dir() {
        panic!(
            "Missing ../SameBoy/Core; did you forget to run `git submodule update --init lab/SameBoy`?"
        );
    }

    // Pull the version string out of SameBoy's version.mk (`VERSION := x.y.z`).
    let version_mk = std::fs::read_to_string("../SameBoy/version.mk")
        .expect("failed to read ../SameBoy/version.mk");
    let version = version_mk
        .split_once(":=")
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
        .unwrap_or("0.0.0");

    let mut build = cc::Build::new();
    build.include(core_path);

    for entry in core_path.read_dir().expect("failed to read SameBoy/Core") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("c") {
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap();
        if SKIP_FILES.contains(&name) {
            continue;
        }
        build.file(&path);
        println!("cargo:rerun-if-changed={}", path.display());
    }

    build.define("GB_INTERNAL", None);
    build.define("GB_DISABLE_DEBUGGER", None);
    build.define("GB_DISABLE_CHEAT_SEARCH", None);
    build.define("GB_VERSION", format!("\"{version}\"").as_str());
    build.warnings(false);
    build.compile("sameboy");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../SameBoy/version.mk");
}
