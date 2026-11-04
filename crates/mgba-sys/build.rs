use std::path::{Path, PathBuf};

const SOURCES: &[(&str, &[&str])] = &[
    (
        "src/arm",
        &[
            "arm.c",
            "decoder-arm.c",
            "decoder.c",
            "decoder-thumb.c",
            "isa-arm.c",
            "isa-thumb.c",
        ],
    ),
    (
        "src/core",
        &[
            "bitmap-cache.c",
            "cache-set.c",
            "cheats.c",
            "config.c",
            "core.c",
            "directories.c",
            "input.c",
            "interface.c",
            "library.c",
            "lockstep.c",
            "log.c",
            "map-cache.c",
            "mem-search.c",
            "rewind.c",
            "serialize.c",
            "sync.c",
            "thread.c",
            "tile-cache.c",
            "timing.c",
        ],
    ),
    (
        "src/gba",
        &[
            "../gb/audio.c",
            "audio.c",
            "bios.c",
            "cart/ereader.c",
            "cart/gpio.c",
            "cart/matrix.c",
            "cart/vfame.c",
            "cheats.c",
            "cheats/codebreaker.c",
            "cheats/gameshark.c",
            "cheats/parv3.c",
            "core.c",
            "dma.c",
            "gba.c",
            "hle-bios.c",
            "input.c",
            "io.c",
            "memory.c",
            "overrides.c",
            "renderers/cache-set.c",
            "renderers/common.c",
            "renderers/software-bg.c",
            "renderers/software-mode0.c",
            "renderers/software-obj.c",
            "renderers/video-software.c",
            "savedata.c",
            "serialize.c",
            "sharkport.c",
            "sio.c",
            "sio/gbp.c",
            "sio/joybus.c",
            "timer.c",
            "video.c",
        ],
    ),
    (
        "src/util",
        &[
            "circle-buffer.c",
            "configuration.c",
            "convolve.c",
            "crc32.c",
            "elf-read.c",
            "export.c",
            "formatting.c",
            "gbk-table.c",
            "hash.c",
            "patch.c",
            "patch-fast.c",
            "patch-ips.c",
            "patch-ups.c",
            "png-io.c",
            "ring-fifo.c",
            "string.c",
            "table.c",
            "text-codec.c",
            "vfs.c",
            "vfs/vfs-mem.c",
            "vfs/vfs-fifo.c",
            "vfs/vfs-fd.c",
            "vfs/vfs-dirent.c",
        ],
    ),
    ("src/platform/posix", &["memory.c"]),
    ("src/third-party/inih", &["ini.c"]),
    ("src/third-party/blip_buf", &["blip_buf.c"]),
];

fn version(root: &Path) -> String {
    let text =
        std::fs::read_to_string(root.join("version.cmake")).expect("failed to read version.cmake");
    let part = |name: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(&format!("set({name} ")))
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap_or("0")
            .to_string()
    };
    format!(
        "{}.{}.{}",
        part("LIB_VERSION_MAJOR"),
        part("LIB_VERSION_MINOR"),
        part("LIB_VERSION_PATCH")
    )
}

fn main() {
    let root = Path::new("mGBA");
    if !root.join("src/gba").is_dir() {
        panic!(
            "Missing mGBA/src; did you forget to run `git submodule update --init crates/mgba-sys/mGBA`?"
        );
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let generated = out.join("include/mgba");
    std::fs::create_dir_all(&generated).expect("failed to create the generated include dir");
    std::fs::write(
        generated.join("flags.h"),
        "#ifndef FLAGS_H\n#define FLAGS_H\n#endif\n",
    )
    .expect("failed to write flags.h");
    let version = version(root);
    let version_c = out.join("version.c");
    std::fs::write(
        &version_c,
        format!(
            "#include <mgba/core/version.h>\n\
             MGBA_EXPORT const char* const gitCommit = \"{version}\";\n\
             MGBA_EXPORT const char* const gitCommitShort = \"{version}\";\n\
             MGBA_EXPORT const char* const gitBranch = \"{version}\";\n\
             MGBA_EXPORT const int gitRevision = -1;\n\
             MGBA_EXPORT const char* const binaryName = \"mgba\";\n\
             MGBA_EXPORT const char* const projectName = \"mGBA\";\n\
             MGBA_EXPORT const char* const projectVersion = \"{version}\";\n"
        ),
    )
    .expect("failed to write version.c");

    let mut build = cc::Build::new();
    build
        .include(out.join("include"))
        .include(root.join("include"))
        .include(root.join("src"));
    for (dir, files) in SOURCES {
        for file in *files {
            let path = root.join(dir).join(file);
            println!("cargo:rerun-if-changed={}", path.display());
            build.file(path);
        }
    }
    build.file(&version_c).file("src/shim.c");
    for define in [
        "M_CORE_GBA",
        "DISABLE_THREADING",
        "MGBA_STANDALONE",
        "BUILD_STATIC",
        "_GNU_SOURCE",
        "_DARWIN_C_SOURCE",
    ] {
        build.define(define, None);
    }
    build.define("MINIMAL_CORE", "2");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "linux" || os == "macos" {
        build
            .define("HAVE_LOCALE", None)
            .define("HAVE_STRTOF_L", None);
    }
    if os == "macos" {
        build.define("HAVE_XLOCALE", None);
    }
    build.warnings(false);
    build.compile("mgba");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-changed=mGBA/version.cmake");
}
