use std::path::Path;

const SKIP_FILES: &[&str] = &[
    "rc_client_external.c",
    "rc_client_raintegration.c",
    "rc_libretro.c",
    "aes.c",
    "cdreader.c",
    "hash_disc.c",
    "hash_encrypted.c",
    "hash_zip.c",
];

const SOURCE_DIRS: &[&str] = &[
    "rcheevos/src",
    "rcheevos/src/rapi",
    "rcheevos/src/rcheevos",
    "rcheevos/src/rhash",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=rcheevos");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }

    let mut build = cc::Build::new();
    build.include("rcheevos/include");
    build.include("rcheevos/src");
    for dir in SOURCE_DIRS {
        let mut files: Vec<_> = Path::new(dir)
            .read_dir()
            .unwrap_or_else(|e| panic!("failed to read {dir}: {e}"))
            .map(|entry| entry.expect("dir entry").path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("c"))
            .filter(|path| {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !SKIP_FILES.contains(&name)
            })
            .collect();
        files.sort();
        build.files(files);
    }

    build.file("glue.c");
    println!("cargo:rerun-if-changed=glue.c");

    for define in [
        "RC_CLIENT_SUPPORTS_HASH",
        "RC_HASH_NO_DISC",
        "RC_HASH_NO_ENCRYPTED",
        "RC_HASH_NO_ZIP",
    ] {
        build.define(define, None);
    }
    build.warnings(false);
    build.compile("rcheevos");
}
