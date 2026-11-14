use std::path::{Path, PathBuf};

const CORE: &[&str] = &[
    "ARCodeFile.cpp",
    "ARDatabaseDAT.cpp",
    "AREngine.cpp",
    "ARM.cpp",
    "ARMInterpreter.cpp",
    "ARMInterpreter_ALU.cpp",
    "ARMInterpreter_Branch.cpp",
    "ARMInterpreter_LoadStore.cpp",
    "CP15.cpp",
    "CRC32.cpp",
    "DMA.cpp",
    "DMA_Timings.cpp",
    "DSi.cpp",
    "DSi_AES.cpp",
    "DSi_Camera.cpp",
    "DSi_DSP.cpp",
    "DSi_I2C.cpp",
    "DSi_I2S.cpp",
    "DSi_NAND.cpp",
    "DSi_NDMA.cpp",
    "DSi_NWifi.cpp",
    "DSi_SD.cpp",
    "DSi_SPI_TSC.cpp",
    "FATIO.cpp",
    "FATStorage.cpp",
    "GBACart.cpp",
    "GBACartMotionPak.cpp",
    "GPU.cpp",
    "GPU2D.cpp",
    "GPU2D_Soft.cpp",
    "GPU3D.cpp",
    "GPU3D_Soft.cpp",
    "GPU3D_Texcache.cpp",
    "Mic.cpp",
    "NDS.cpp",
    "NDSCart.cpp",
    "NDSCartR4.cpp",
    "ROMList.cpp",
    "FreeBIOS.cpp",
    "RTC.cpp",
    "Savestate.cpp",
    "SPI.cpp",
    "SPI_Firmware.cpp",
    "SPU.cpp",
    "Utils.cpp",
    "Wifi.cpp",
    "WifiAP.cpp",
    "DSP_HLE/UcodeBase.cpp",
    "DSP_HLE/AACUcode.cpp",
    "DSP_HLE/G711Ucode.cpp",
    "DSP_HLE/GraphicsUcode.cpp",
];

const TEAKRA: &[&str] = &[
    "ahbm.cpp",
    "apbp.cpp",
    "btdmp.cpp",
    "disassembler.cpp",
    "dma.cpp",
    "timer.cpp",
    "memory_interface.cpp",
    "mmio.cpp",
    "parser.cpp",
    "processor.cpp",
    "teakra.cpp",
];

const C: &[&str] = &[
    "fatfs/ff.c",
    "fatfs/ffsystem.c",
    "fatfs/ffunicode.c",
    "sha1/sha1.c",
    "tiny-AES-c/aes.c",
    "xxhash/xxhash.c",
    "blip-buf/blip_buf.c",
];

const JIT: &[&str] = &[
    "ARM_InstrInfo.cpp",
    "ARMJIT.cpp",
    "ARMJIT_Memory.cpp",
    "ARMJIT_Global.cpp",
    "dolphin/CommonFuncs.cpp",
];

const JIT_X64: &[&str] = &[
    "dolphin/x64ABI.cpp",
    "dolphin/x64CPUDetect.cpp",
    "dolphin/x64Emitter.cpp",
    "ARMJIT_x64/ARMJIT_Compiler.cpp",
    "ARMJIT_x64/ARMJIT_ALU.cpp",
    "ARMJIT_x64/ARMJIT_LoadStore.cpp",
    "ARMJIT_x64/ARMJIT_Branch.cpp",
];

const JIT_A64: &[&str] = &[
    "dolphin/Arm64Emitter.cpp",
    "dolphin/MathUtil.cpp",
    "ARMJIT_A64/ARMJIT_Compiler.cpp",
    "ARMJIT_A64/ARMJIT_ALU.cpp",
    "ARMJIT_A64/ARMJIT_LoadStore.cpp",
    "ARMJIT_A64/ARMJIT_Branch.cpp",
];

fn jit_arch() -> Option<(&'static [&'static str], &'static str)> {
    std::env::var_os("CARGO_FEATURE_JIT")?;
    match std::env::var("CARGO_CFG_TARGET_ARCH").ok()?.as_str() {
        "x86_64" => Some((JIT_X64, "ARMJIT_x64/ARMJIT_Linkage.S")),
        "aarch64" => Some((JIT_A64, "ARMJIT_A64/ARMJIT_Linkage.S")),
        _ => None,
    }
}

fn version(root: &Path) -> String {
    let text = std::fs::read_to_string(root.join("CMakeLists.txt"))
        .expect("failed to read CMakeLists.txt");
    text.lines()
        .find_map(|line| line.trim().strip_prefix("VERSION "))
        .expect("no VERSION line in CMakeLists.txt")
        .trim()
        .to_string()
}

fn add(build: &mut cc::Build, dir: &Path, files: &[&str]) {
    for file in files {
        let path = dir.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        build.file(path);
    }
}

fn main() {
    let root = Path::new("melonDS");
    let src = root.join("src");
    if !src.join("NDS.cpp").is_file() {
        panic!(
            "Missing melonDS/src; did you forget to run `git submodule update --init crates/melonds-sys/melonDS`?"
        );
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let version = version(root);
    std::fs::write(
        out.join("version.h"),
        format!(
            "#ifndef VERSION_H\n#define VERSION_H\n\
             #define MELONDS_URL \"https://melonds.kuribo64.net\"\n\
             #define MELONDS_VERSION_BASE \"{version}\"\n\
             #define MELONDS_VERSION_SUFFIX \"\"\n\
             #define MELONDS_VERSION MELONDS_VERSION_BASE MELONDS_VERSION_SUFFIX\n\
             #endif\n"
        ),
    )
    .expect("failed to write version.h");

    let jit = jit_arch();
    let mut c = cc::Build::new();
    c.include(&src).warnings(false);
    add(&mut c, &src, C);
    if let Some((_, linkage)) = jit {
        c.define("JIT_ENABLED", None);
        add(&mut c, &src, &[linkage]);
    }
    c.compile("melonds_c");

    let mut cpp = cc::Build::new();
    cpp.cpp(true)
        .std("c++17")
        .include(&out)
        .include(&src)
        .include(src.join("teakra/include"))
        .include(src.join("teakra/src"))
        .warnings(false);
    add(&mut cpp, &src, CORE);
    add(&mut cpp, &src.join("teakra/src"), TEAKRA);
    if let Some((arch, _)) = jit {
        cpp.define("JIT_ENABLED", None);
        add(&mut cpp, &src, JIT);
        add(&mut cpp, &src, arch);
    }
    cpp.file("src/shim.cpp");
    cpp.compile("melonds");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-changed=melonDS/CMakeLists.txt");
}
